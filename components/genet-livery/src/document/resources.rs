// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

//! Host-supplied image and font bytes.
//!
//! The document owns the ledgers; the host replaces them wholesale or
//! one entry at a time, and a font change rebuilds Parley's database.

use super::*;

impl<D> LiveryDocument<D>
where
    D: LayoutDom,
    D::NodeId: Copy + Eq + Hash,
{
    /// Supply host-resolved image bytes for a non-data URL. The CSS engine
    /// still owns decoding and paint-key allocation; the host owns URL
    /// resolution and fetching.
    pub fn set_image_resource(&mut self, url: impl Into<String>, bytes: Vec<u8>) {
        let url = url.into();
        if self.image_sources.get(&url) == Some(&bytes) {
            return;
        }
        self.image_sources.insert(url, bytes);
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Supply host-resolved font bytes for a non-data URL. The host owns URL
    /// resolution and fetching. A source identity replaces prior bytes rather
    /// than registering another competing face in Parley's collection.
    pub fn set_font_resource(&mut self, url: impl Into<String>, bytes: Vec<u8>) {
        let url = url.into();
        if self.font_sources.get(&url) == Some(&bytes) {
            return;
        }
        self.font_sources.insert(url, bytes);
        self.rebuild_font_resources();
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Replace the complete host image ledger. A missing prior key is removed,
    /// so a failed or deleted live image cannot remain painted from stale bytes.
    pub fn replace_image_resources(
        &mut self,
        resources: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) {
        let next = resources.into_iter().collect::<HashMap<_, _>>();
        if self.image_sources == next {
            return;
        }
        self.image_sources = next;
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    /// Replace the complete host font ledger. Fontique has no per-blob removal
    /// operation, so a changed or removed source rebuilds this document's font
    /// context from the surviving ledger before the next layout.
    pub fn replace_font_resources(
        &mut self,
        resources: impl IntoIterator<Item = (String, Vec<u8>)>,
    ) {
        let next = resources.into_iter().collect::<HashMap<_, _>>();
        if self.font_sources == next {
            return;
        }
        self.font_sources = next;
        self.rebuild_font_resources();
        self.invalidate_with_layout_damage(LayoutDamageKind::Resource);
    }

    pub(in crate::document) fn rebuild_font_resources(&mut self) {
        self.text = TextSystem::new();
        let mut face_sources = HashSet::new();
        for face in self.style_set.font_faces() {
            if !face.is_host_loadable() {
                continue;
            }
            let Some((source, bytes)) = face.sources().iter().find_map(|source| {
                self.font_sources
                    .get(source.as_ref())
                    .map(|bytes| (source.as_ref(), bytes))
            }) else {
                continue;
            };
            face_sources.insert(source.to_owned());
            self.text.register_font_face_bytes(
                bytes.clone(),
                face.family(),
                face.feature_settings(),
            );
        }
        for (source, bytes) in &self.font_sources {
            if !face_sources.contains(source) {
                self.text.register_font_bytes(bytes.clone());
            }
        }
    }
}
