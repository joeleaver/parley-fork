// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `Collection::system_render_properties` on fontconfig: the rendering
//! properties of the matched font, from the configuration the scan was made
//! with.
//!
//! fontconfig reads `FONTCONFIG_FILE` when a configuration is loaded, so this
//! test names one of its own, with one font and rules of both targets: a
//! `target="pattern"` rule and a `target="font"` rule that tests the matched
//! font's family, which only a match against the scanned fonts applies. It is
//! its own test binary, so the variable is set before anything in the process
//! loads a configuration, and it reads no font of the host's.

#![cfg(all(feature = "system", any(target_os = "linux", target_os = "freebsd")))]

use fontique::{Collection, CollectionOptions};

const ROBOTO: &[u8] =
    include_bytes!("../../parley_dev/assets/fonts/roboto_fonts/Roboto-Regular.ttf");

#[test]
fn the_properties_are_those_of_the_matched_font() {
    let dir = std::env::temp_dir().join(format!("fontique-render-{}", std::process::id()));
    let fonts = dir.join("fonts");
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("Roboto-Regular.ttf"), ROBOTO).unwrap();
    let conf = dir.join("fonts.conf");
    std::fs::write(
        &conf,
        format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n\
             <fontconfig><dir>{}</dir><cachedir>{}</cachedir>\
             <match target=\"pattern\">\
               <edit name=\"hintstyle\" mode=\"assign\"><const>hintmedium</const></edit>\
               <edit name=\"lcdfilter\" mode=\"assign\"><const>lcdlight</const></edit>\
             </match>\
             <match target=\"font\">\
               <test name=\"family\"><string>Roboto</string></test>\
               <edit name=\"rgba\" mode=\"assign\"><const>bgr</const></edit>\
               <edit name=\"antialias\" mode=\"assign\"><bool>false</bool></edit>\
             </match>\
             <match target=\"font\">\
               <test name=\"family\"><string>Not Installed</string></test>\
               <edit name=\"hinting\" mode=\"assign\"><bool>false</bool></edit>\
             </match>\
             </fontconfig>\n",
            fonts.display(),
            dir.join("cache").display()
        ),
    )
    .unwrap();
    // SAFETY: this binary's only test, run before any other thread of its
    // own reads the environment or loads fontconfig.
    #[expect(
        unsafe_code,
        reason = "fontconfig takes its configuration file from the environment"
    )]
    unsafe {
        std::env::set_var("FONTCONFIG_FILE", &conf);
    }

    let collection = Collection::new(CollectionOptions::default());
    if !collection.system_font_paths().config_files.contains(&conf) {
        // No libfontconfig on this host (the `fontconfig-dlopen` build).
        eprintln!("libfontconfig not found; skipped");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let properties = collection
        .system_render_properties("sans-serif")
        .expect("the one font matches");
    // fontconfig's `FC_RGBA_BGR`, `FC_HINT_MEDIUM`, `FC_LCD_LIGHT`.
    assert_eq!(properties.rgba, Some(2), "{properties:?}");
    assert_eq!(properties.antialias, Some(false), "{properties:?}");
    assert_eq!(properties.hint_style, Some(2), "{properties:?}");
    assert_eq!(properties.lcd_filter, Some(2), "{properties:?}");
    // `FcDefaultSubstitute`'s, untouched by the rule for a font not matched.
    assert_eq!(properties.hinting, Some(true), "{properties:?}");

    // A clone shares the scan, so it answers the same.
    assert_eq!(
        collection.clone().system_render_properties("sans-serif"),
        Some(properties)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
