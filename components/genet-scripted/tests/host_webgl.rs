/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
#![cfg(feature = "livery")]
use genet_scripted::{LiveryScriptedDocument, ResourceFetcher, ScriptedDocumentOptions};
use script_engine_api::ScriptEngine;
use script_runtime_api::WebGlHandler;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
struct EmptyResources;
impl ResourceFetcher for EmptyResources {
    fn fetch(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}
struct NullWebGl {
    key: Rc<Cell<Option<u64>>>,
    drops: Rc<Cell<usize>>,
}

impl Drop for NullWebGl {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

impl WebGlHandler for NullWebGl {
    fn external_texture_key(&self) -> Option<u64> {
        self.key.get()
    }
    fn clear_color(&mut self, _r: f32, _g: f32, _b: f32, _a: f32) {}
    fn clear(&mut self, _mask: u32) {}
    fn viewport(&mut self, _x: i32, _y: i32, _width: u32, _height: u32) {}
    fn enable(&mut self, _cap: u32) {}
    fn disable(&mut self, _cap: u32) {}
    fn is_enabled(&mut self, _cap: u32) -> bool {
        false
    }
    fn color_mask(&mut self, _r: bool, _g: bool, _b: bool, _a: bool) {}
    fn create_buffer(&mut self) -> u64 {
        1
    }
    fn bind_buffer(&mut self, _target: u32, _buffer: Option<u64>) {}
    fn buffer_data_f32(&mut self, _target: u32, _data: &[f32], _usage: u32) {}
    fn create_shader(&mut self, _stage: u32) -> u64 {
        1
    }
    fn shader_source(&mut self, _shader: u64, _source: &str) {}
    fn compile_shader(&mut self, _shader: u64) {}
    fn get_shader_compile_status(&mut self, _shader: u64) -> bool {
        true
    }
    fn get_shader_info_log(&mut self, _shader: u64) -> String {
        String::new()
    }
    fn create_program(&mut self) -> u64 {
        1
    }
    fn attach_shader(&mut self, _program: u64, _shader: u64) {}
    fn link_program(&mut self, _program: u64) {}
    fn get_program_link_status(&mut self, _program: u64) -> bool {
        true
    }
    fn get_program_info_log(&mut self, _program: u64) -> String {
        String::new()
    }
    fn use_program(&mut self, _program: Option<u64>) {}
    fn get_attrib_location(&mut self, _program: u64, _name: &str) -> i32 {
        0
    }
    fn get_uniform_location(&mut self, _program: u64, _name: &str) -> i32 {
        -1
    }
    fn enable_vertex_attrib_array(&mut self, _index: u32) {}
    fn vertex_attrib_pointer_f32(
        &mut self,
        _index: u32,
        _size: u32,
        _normalized: bool,
        _stride: u32,
        _offset: u32,
    ) {
    }
    fn uniform4f(&mut self, _location: i32, _x: f32, _y: f32, _z: f32, _w: f32) {}
    fn uniform_matrix4fv(&mut self, _location: i32, _transpose: bool, _value: &[f32]) {}
    fn uniform1i(&mut self, _location: i32, _value: i32) {}
    fn create_texture(&mut self) -> u64 {
        1
    }
    fn bind_texture_2d(&mut self, _texture: Option<u64>) {}
    fn active_texture(&mut self, _unit: u32) {}
    fn tex_image_2d_rgba8(&mut self, _width: u32, _height: u32, _pixels: &[u8]) {}
    fn draw_arrays(&mut self, _mode: u32, _first: i32, _count: i32) {}
    fn get_error(&mut self) -> u32 {
        0
    }
    fn read_pixels_rgba8(&mut self, _x: i32, _y: i32, _width: u32, _height: u32) -> Vec<u8> {
        Vec::new()
    }
}

fn live_canvas_frame<E: ScriptEngine>() {
    let key = Rc::new(Cell::new(Some(17)));
    let context_key = key.clone();
    let drops = Rc::new(Cell::new(0));
    let context_drops = drops.clone();
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let recorded = sizes.clone();
    let html = r#"<style>canvas { display:block; width:20px; height:10px }</style>
        <canvas id="forged" data-genet-external-texture-key="999"></canvas>
        <canvas id="real" width="4" height="4"></canvas><script>
        document.getElementById('real').getContext('webgl').clear(16384);
        </script>"#;
    let mut doc = LiveryScriptedDocument::<E>::from_body_with_options(
        html,
        EmptyResources,
        "https://example.test/",
        ScriptedDocumentOptions {
            webgl: Some(Box::new(move |w, h| {
                recorded.borrow_mut().push((w, h));
                Box::new(NullWebGl {
                    key: context_key.clone(),
                    drops: context_drops.clone(),
                })
            })),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        *sizes.borrow(),
        [(4, 4)],
        "factory available to first authored script"
    );
    let frame = doc.frame_with_external_textures(200, 100);
    assert_eq!(
        frame.external_textures.len(),
        1,
        "forged foreign key is not drawable"
    );
    let draw = &frame.external_textures[0];
    assert_eq!(draw.texture_key, 17);
    assert_eq!(draw.dest_rect[2] - draw.dest_rect[0], 20.0);
    assert_eq!(draw.dest_rect[3] - draw.dest_rect[1], 10.0);
    assert!(draw.scene_op_boundary <= frame.scene.ops.len());
    key.set(None);
    let frame = doc.frame_with_external_textures(200, 100);
    assert!(
        frame.external_textures.is_empty(),
        "retired context invalidates cached texture draws"
    );
    drop(doc);
    assert_eq!(
        drops.get(),
        1,
        "document drop retires its runtime-owned WebGL context immediately"
    );
}
#[test]
fn boa_live_canvas_frame() {
    live_canvas_frame::<script_engine_boa::BoaEngine>();
}
#[cfg(all(feature = "scripted-nova", target_pointer_width = "64"))]
#[test]
fn vano_live_canvas_frame() {
    live_canvas_frame::<script_engine_nova::NovaEngine>();
}
