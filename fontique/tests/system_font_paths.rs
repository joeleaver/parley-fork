// Copyright 2026 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! `Collection::system_font_paths` on fontconfig: the lists of the
//! configuration the scan was made with.
//!
//! fontconfig reads `FONTCONFIG_FILE` when a configuration is loaded, so this
//! test names one of its own: a font directory with a subdirectory, one that
//! does not exist, and an included directory of configuration files. It is
//! its own test binary, so the variable is set before anything in the process
//! loads a configuration, and it reads no font of the host's.

#![cfg(all(feature = "system", any(target_os = "linux", target_os = "freebsd")))]

use fontique::{Collection, CollectionOptions};

const ROBOTO: &[u8] =
    include_bytes!("../../parley_dev/assets/fonts/roboto_fonts/Roboto-Regular.ttf");

#[test]
fn the_paths_are_those_of_the_scanned_configuration() {
    let dir = std::env::temp_dir().join(format!("fontique-paths-{}", std::process::id()));
    let fonts = dir.join("fonts");
    let sub = fonts.join("sub");
    let missing = dir.join("missing");
    let conf_d = dir.join("conf.d");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::create_dir_all(&conf_d).unwrap();
    std::fs::write(sub.join("Roboto-Regular.ttf"), ROBOTO).unwrap();
    let conf = dir.join("fonts.conf");
    std::fs::write(
        &conf,
        format!(
            "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"fonts.dtd\">\n\
             <fontconfig><dir>{}</dir><dir>{}</dir><include>{}</include>\
             <cachedir>{}</cachedir></fontconfig>\n",
            fonts.display(),
            missing.display(),
            conf_d.display(),
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
    let paths = collection.system_font_paths();
    if paths == fontique::SystemFontPaths::default() {
        // No libfontconfig on this host (the `fontconfig-dlopen` build).
        eprintln!("libfontconfig not found; skipped");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert!(paths.config_files.contains(&conf), "{paths:?}");
    assert!(
        paths.config_dirs.contains(&conf_d),
        "an included directory of configuration files is listed: {paths:?}"
    );
    assert!(paths.font_dirs.contains(&fonts), "{paths:?}");
    assert!(
        paths.font_dirs.contains(&sub),
        "a scanned subdirectory is listed: {paths:?}"
    );
    assert!(
        paths.font_dirs.contains(&missing),
        "a configured font directory that does not exist is listed: {paths:?}"
    );

    // A clone shares the scan, so it answers the same lists.
    assert_eq!(collection.clone().system_font_paths(), paths);
    let _ = std::fs::remove_dir_all(&dir);
}
