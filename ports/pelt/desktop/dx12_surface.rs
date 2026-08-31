//! D3D12 shared-texture import at Pelt's desktop composition boundary.

use std::collections::HashMap;
use std::ffi::c_void;

use inker::{
    FrameHandleOwnership, NativeTextureHandle, SurfaceFrame, SurfaceSyncHandle,
    SurfaceTextureFormat,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FENCE_FLAG_SHARED, D3D12_RESOURCE_DIMENSION_TEXTURE2D,
    D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
};
use workbench::TileId;

pub(crate) struct Dx12SurfaceCache {
    textures: HashMap<TileId, ImportedSurface>,
    frames: u32,
    imports: u32,
    waits: u32,
    compositions: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Dx12SurfaceStats {
    pub(crate) frames: u32,
    pub(crate) imports: u32,
    pub(crate) waits: u32,
    pub(crate) compositions: u32,
}

pub(crate) struct Dx12SharedFence {
    _fence: ID3D12Fence,
    shared_handle: HANDLE,
}

// The COM fence is free-threaded and the HANDLE is an opaque process value.
unsafe impl Send for Dx12SharedFence {}
unsafe impl Sync for Dx12SharedFence {}

impl Dx12SharedFence {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self, String> {
        // SAFETY: the HAL guard is used only to create a shared fence on this
        // host-owned D3D12 device.
        unsafe {
            let hal_device = device
                .as_hal::<wgpu::wgc::api::Dx12>()
                .ok_or_else(|| "Pelt cannot create a fence on a non-D3D12 device".to_owned())?;
            let d3d_device = hal_device.raw_device();
            let fence = d3d_device
                .CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_SHARED)
                .map_err(|error| format!("CreateFence failed: {error}"))?;
            let shared_handle = d3d_device
                .CreateSharedHandle(&fence, None, GENERIC_ALL.0, None)
                .map_err(|error| format!("CreateSharedHandle(fence) failed: {error}"))?;
            Ok(Self {
                _fence: fence,
                shared_handle,
            })
        }
    }

    pub(crate) fn raw_handle(&self) -> u64 {
        self.shared_handle.0 as usize as u64
    }
}

impl Drop for Dx12SharedFence {
    fn drop(&mut self) {
        if !self.shared_handle.is_invalid() {
            // SAFETY: this object owns the handle returned by CreateSharedHandle.
            unsafe {
                let _ = CloseHandle(self.shared_handle);
            }
        }
    }
}

struct ImportedSurface {
    epoch: u64,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    fence: Option<(u64, ID3D12Fence)>,
    pending_wait: Option<u64>,
}

type OpenedFence = (u64, ID3D12Fence);
type PendingFenceWait = (u64, u64);
type OpenedSync = (Option<OpenedFence>, Option<u64>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourceAction {
    Reuse,
    Import,
}

struct IncomingTextureHandle {
    raw: u64,
    close_on_drop: bool,
}

impl IncomingTextureHandle {
    fn new(raw: u64, ownership: FrameHandleOwnership) -> Self {
        Self {
            raw,
            close_on_drop: ownership == FrameHandleOwnership::Transferred,
        }
    }

    fn as_windows(&self) -> HANDLE {
        HANDLE(self.raw as usize as *mut c_void)
    }
}

impl Drop for IncomingTextureHandle {
    fn drop(&mut self) {
        if self.close_on_drop && self.raw != 0 {
            // SAFETY: `Transferred` gives this one-shot Win32 handle to the host.
            // Opening the resource retains a COM reference, so the OS handle can
            // be closed on both the success and rejection paths.
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(self.raw as _);
            }
        }
    }
}

impl Dx12SurfaceCache {
    pub(crate) fn new() -> Self {
        Self {
            textures: HashMap::new(),
            frames: 0,
            imports: 0,
            waits: 0,
            compositions: 0,
        }
    }

    pub(crate) fn accept_frame(
        &mut self,
        tile: TileId,
        frame: SurfaceFrame,
        device: &wgpu::Device,
    ) -> Result<(), String> {
        let SurfaceFrame {
            texture,
            sync,
            width,
            height,
            format,
            resource_epoch,
        } = frame;
        self.frames = self.frames.saturating_add(1);
        let (raw, ownership) = match texture {
            NativeTextureHandle::D3d12Shared { handle, ownership } => (handle, ownership),
            other => return Err(format!("Pelt's Windows importer cannot open {other:?}")),
        };
        let handle = IncomingTextureHandle::new(raw, ownership);
        if width == 0 || height == 0 {
            return Err(format!(
                "D3D12 surface frame declared an empty {width}x{height} texture"
            ));
        }
        let (format, dxgi_format) = map_format(&format)?;
        let pending_wait = match sync {
            SurfaceSyncHandle::D3d12Fence { handle, value } if handle != 0 && value != 0 => {
                Some((handle, value))
            },
            SurfaceSyncHandle::D3d12Fence { handle, value } => {
                return Err(format!(
                    "D3D12 surface frame carried an invalid fence handle/value ({handle:#x}, {value})"
                ));
            },
            SurfaceSyncHandle::None => None,
            other => return Err(format!("Pelt's D3D12 importer cannot wait on {other:?}")),
        };

        let cached_epoch = self.textures.get(&tile).map(|surface| surface.epoch);
        if resource_action(cached_epoch, resource_epoch, handle.raw)? == ResourceAction::Reuse {
            let cached = self
                .textures
                .get_mut(&tile)
                .expect("reuse requires the cached epoch above");
            if (cached.width, cached.height, cached.format) != (width, height, format) {
                return Err(format!(
                    "tile {} reused resource epoch {} with changed texture metadata",
                    tile.0, resource_epoch
                ));
            }
            update_sync(cached, device, pending_wait)?;
            return Ok(());
        }

        let (fence, pending_wait) = open_sync(device, pending_wait)?;
        let texture = import_texture(device, &handle, width, height, format, dxgi_format)?;
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("Pelt imported D3D12 surface view"),
            format: Some(format),
            ..Default::default()
        });
        self.textures.insert(
            tile,
            ImportedSurface {
                epoch: resource_epoch,
                width,
                height,
                format,
                texture,
                view,
                fence,
                pending_wait,
            },
        );
        self.imports = self.imports.saturating_add(1);
        Ok(())
    }

    pub(crate) fn stage_wait(&mut self, tile: TileId, queue: &wgpu::Queue) -> Result<(), String> {
        let Some(surface) = self.textures.get_mut(&tile) else {
            return Ok(());
        };
        let Some(value) = surface.pending_wait.take() else {
            return Ok(());
        };
        let fence = surface
            .fence
            .as_ref()
            .map(|(_, fence)| fence.clone())
            .ok_or_else(|| format!("tile {} has a fence value without an opened fence", tile.0))?;
        // SAFETY: the queue and fence both belong to this D3D12 device. The HAL
        // stages Wait before the next submit, which is Pelt's sampling pass.
        let hal_queue = unsafe { queue.as_hal::<wgpu::wgc::api::Dx12>() }
            .ok_or_else(|| "Pelt cannot sample a D3D12 frame on a non-D3D12 queue".to_owned())?;
        hal_queue.add_wait_fence(fence, value);
        self.waits = self.waits.saturating_add(1);
        Ok(())
    }

    pub(crate) fn view(&self, tile: TileId) -> Option<&wgpu::TextureView> {
        self.textures.get(&tile).map(|surface| &surface.view)
    }

    pub(crate) fn dimensions(&self, tile: TileId) -> Option<(u32, u32)> {
        self.textures
            .get(&tile)
            .map(|surface| (surface.width, surface.height))
    }

    pub(crate) fn retain_tiles(&mut self, mut keep: impl FnMut(TileId) -> bool) {
        self.textures.retain(|tile, _| keep(*tile));
    }

    pub(crate) fn mark_composed(&mut self) {
        self.compositions = self.compositions.saturating_add(1);
    }

    pub(crate) fn return_to_common(
        &self,
        tile: TileId,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), String> {
        let surface = self
            .textures
            .get(&tile)
            .ok_or_else(|| format!("tile {} has no imported D3D12 texture", tile.0))?;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Pelt D3D12 surface return-to-COMMON encoder"),
        });
        encoder.transition_resources(
            std::iter::empty::<wgpu::BufferTransition<&wgpu::Buffer>>(),
            std::iter::once(wgpu::TextureTransition {
                texture: &surface.texture,
                selector: None,
                state: wgpu::TextureUses::empty(),
            }),
        );
        queue.submit([encoder.finish()]);
        // Scrying performs the next D3D11 CopyResource inside the next
        // `acquire_frame` call. Finish this sample and COMMON transition before
        // the workspace asks it to overwrite the persistent shared texture.
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("waiting for native surface release failed: {error}"))?;
        Ok(())
    }

    pub(crate) fn stats(&self) -> Dx12SurfaceStats {
        Dx12SurfaceStats {
            frames: self.frames,
            imports: self.imports,
            waits: self.waits,
            compositions: self.compositions,
        }
    }
}

fn resource_action(
    cached_epoch: Option<u64>,
    incoming_epoch: u64,
    handle: u64,
) -> Result<ResourceAction, String> {
    if cached_epoch == Some(incoming_epoch) && handle == 0 {
        return Ok(ResourceAction::Reuse);
    }
    if handle != 0 {
        return Ok(ResourceAction::Import);
    }
    Err(format!(
        "D3D12 surface frame introduced resource epoch {incoming_epoch} without a texture handle"
    ))
}

fn open_sync(device: &wgpu::Device, sync: Option<PendingFenceWait>) -> Result<OpenedSync, String> {
    let Some((handle, value)) = sync else {
        return Ok((None, None));
    };
    Ok((Some((handle, open_fence(device, handle)?)), Some(value)))
}

fn update_sync(
    surface: &mut ImportedSurface,
    device: &wgpu::Device,
    sync: Option<PendingFenceWait>,
) -> Result<(), String> {
    let Some((handle, value)) = sync else {
        surface.pending_wait = None;
        return Ok(());
    };
    if surface
        .fence
        .as_ref()
        .is_none_or(|(opened_handle, _)| *opened_handle != handle)
    {
        surface.fence = Some((handle, open_fence(device, handle)?));
    }
    surface.pending_wait = Some(value);
    Ok(())
}

fn map_format(format: &SurfaceTextureFormat) -> Result<(wgpu::TextureFormat, DXGI_FORMAT), String> {
    match format {
        SurfaceTextureFormat::Rgba8Unorm => {
            Ok((wgpu::TextureFormat::Rgba8Unorm, DXGI_FORMAT_R8G8B8A8_UNORM))
        },
        SurfaceTextureFormat::Rgba8UnormSrgb => Ok((
            wgpu::TextureFormat::Rgba8UnormSrgb,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        )),
        SurfaceTextureFormat::Bgra8Unorm => {
            Ok((wgpu::TextureFormat::Bgra8Unorm, DXGI_FORMAT_B8G8R8A8_UNORM))
        },
        SurfaceTextureFormat::Bgra8UnormSrgb => Ok((
            wgpu::TextureFormat::Bgra8UnormSrgb,
            DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        )),
        SurfaceTextureFormat::Other(name) => {
            Err(format!("unsupported native surface texture format {name}"))
        },
    }
}

fn import_texture(
    device: &wgpu::Device,
    handle: &IncomingTextureHandle,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    dxgi_format: DXGI_FORMAT,
) -> Result<wgpu::Texture, String> {
    // SAFETY: the HAL guard is used only to open a resource on the same device.
    // The native descriptor is checked before it is wrapped for wgpu.
    unsafe {
        let hal_device = device
            .as_hal::<wgpu::wgc::api::Dx12>()
            .ok_or_else(|| "Pelt cannot import a D3D12 frame on a non-D3D12 device".to_owned())?;
        let d3d_device = hal_device.raw_device().clone();
        let mut resource: Option<ID3D12Resource> = None;
        d3d_device
            .OpenSharedHandle(handle.as_windows(), &mut resource)
            .map_err(|error| format!("OpenSharedHandle(texture) failed: {error}"))?;
        let resource =
            resource.ok_or_else(|| "OpenSharedHandle(texture) returned no resource".to_owned())?;
        let native = resource.GetDesc();
        if native.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
            || native.Width != u64::from(width)
            || native.Height != height
            || native.DepthOrArraySize != 1
            || native.MipLevels != 1
            || native.SampleDesc.Count != 1
            || native.Format != dxgi_format
            || !native
                .Flags
                .contains(D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS)
        {
            return Err(format!(
                "shared D3D12 resource descriptor did not match the frame: native={:?} declared={}x{} {:?}",
                native, width, height, format
            ));
        }
        drop(hal_device);

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
            resource,
            format,
            wgpu::TextureDimension::D2,
            size,
            1,
            1,
        );
        let descriptor = wgpu::TextureDescriptor {
            label: Some("Pelt imported D3D12 surface"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // The Inker D3D12 frame contract requires COMMON at handoff. In wgpu's
        // native state vocabulary COMMON is the empty TextureUses bitset.
        Ok(device.create_texture_from_hal::<wgpu::wgc::api::Dx12>(
            hal_texture,
            &descriptor,
            wgpu::TextureUses::empty(),
        ))
    }
}

fn open_fence(device: &wgpu::Device, handle: u64) -> Result<ID3D12Fence, String> {
    // SAFETY: the borrowed shared handle is opened on the same D3D12 device and
    // retained as a COM reference. The producer retains ownership of the handle.
    unsafe {
        let hal_device = device
            .as_hal::<wgpu::wgc::api::Dx12>()
            .ok_or_else(|| "Pelt cannot open a D3D12 fence on a non-D3D12 device".to_owned())?;
        let mut fence = None;
        hal_device
            .raw_device()
            .OpenSharedHandle::<ID3D12Fence>(HANDLE(handle as usize as *mut c_void), &mut fence)
            .map_err(|error| format!("OpenSharedHandle(fence) failed: {error}"))?;
        fence.ok_or_else(|| "OpenSharedHandle(fence) returned no fence".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{ResourceAction, map_format, resource_action};
    use inker::SurfaceTextureFormat;
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM,
    };

    #[test]
    fn maps_declared_format_exactly() {
        assert_eq!(
            map_format(&SurfaceTextureFormat::Rgba8Unorm),
            Ok((wgpu::TextureFormat::Rgba8Unorm, DXGI_FORMAT_R8G8B8A8_UNORM))
        );
        assert_eq!(
            map_format(&SurfaceTextureFormat::Bgra8UnormSrgb),
            Ok((
                wgpu::TextureFormat::Bgra8UnormSrgb,
                DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            ))
        );
        assert!(map_format(&SurfaceTextureFormat::Other("nv12".into())).is_err());
    }

    #[test]
    fn only_a_cached_epoch_may_omit_its_texture_handle() {
        assert_eq!(resource_action(Some(7), 7, 0), Ok(ResourceAction::Reuse));
        assert_eq!(resource_action(Some(7), 7, 42), Ok(ResourceAction::Import));
        assert!(resource_action(Some(7), 8, 0).is_err());
        assert!(resource_action(None, 1, 0).is_err());
    }
}
