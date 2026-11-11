// Copyright 2025 the Parley Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
    ptr::NonNull,
};
use std::{
    borrow::Cow,
    ffi::{CString, OsStr},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::Arc,
    vec::Vec,
};

#[cfg(feature = "fontconfig-dlopen")]
use fontconfig_sys::statics::{LIB, LIB_RESULT};
use fontconfig_sys::{
    FcChar8, FcCharSet, FcConfig, FcFontSet, FcLangSet, FcMatchKind, FcMatchPattern, FcPattern,
    FcResult, FcResultMatch, FcResultNoId, FcResultNoMatch, FcResultOutOfMemory,
    FcResultTypeMismatch, FcSetSystem, FcStrList,
    constants::{
        FC_CHARSET, FC_FAMILY, FC_FILE, FC_INDEX, FC_LANG, FC_SCALABLE, FC_SLANT, FC_WEIGHT,
        FC_WIDTH,
    },
    ffi_dispatch,
};
#[cfg(not(feature = "fontconfig-dlopen"))]
use fontconfig_sys::{
    FcCharSetAddChar, FcCharSetCopy, FcCharSetCreate, FcCharSetDestroy, FcConfigBuildFonts,
    FcConfigDestroy, FcConfigGetConfigDirs, FcConfigGetConfigFiles, FcConfigGetFontDirs,
    FcConfigGetFonts, FcConfigReference, FcConfigSubstitute, FcDefaultSubstitute, FcFontMatch,
    FcFontRenderPrepare, FcFontSetDestroy, FcFontSort, FcInitLoadConfig, FcLangSetAdd,
    FcLangSetCopy, FcLangSetCreate, FcLangSetDestroy, FcNameUnparse, FcPatternAddBool,
    FcPatternAddCharSet, FcPatternAddLangSet, FcPatternAddString, FcPatternCreate,
    FcPatternDestroy, FcPatternGetInteger, FcPatternGetString, FcPatternReference, FcStrFree,
    FcStrListDone, FcStrListNext,
};

use hashbrown::{HashMap, HashSet, hash_map::Entry};
use smallvec::SmallVec;

use crate::{
    FallbackKey, FamilyId, FamilyInfo, FontInfo, FontStyle, FontWeight, FontWidth,
    FromFontconfig as _, GenericFamily, Script, ScriptExt,
    family_name::{FamilyName, FamilyNameMap},
    generic::GenericFamilyMap,
    source::SourcePathMap,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MatchErr {
    NoMatch,
    TypeMismatch,
    NoId,
    OutOfMemory,
    Other,
}

impl MatchErr {
    fn from_raw(raw: FcResult) -> Self {
        #[allow(non_upper_case_globals)]
        match raw {
            FcResultNoMatch => Self::NoMatch,
            FcResultTypeMismatch => Self::TypeMismatch,
            FcResultNoId => Self::NoId,
            FcResultOutOfMemory => Self::OutOfMemory,
            _ => Self::Other,
        }
    }
}

type MatchResult<T> = Result<T, MatchErr>;

/// Ownership for refcounted Fontconfig objects. Used to track if a given
/// fontconfig function returns an object that it owns or is passing its
/// ownership to us.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ownership {
    /// This object is owned by Fontconfig and needs to be freed by it.
    Fontconfig,
    /// This object is owned by the application.
    Application,
}

/// Wrapper for an `FcPattern`.
struct Pattern {
    inner: NonNull<FcPattern>,
}

impl Pattern {
    fn new() -> Option<Self> {
        Some(unsafe {
            Self::from_raw(
                ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcPatternCreate,),
                Ownership::Application,
            )?
        })
    }

    unsafe fn from_raw(raw: *mut FcPattern, ownership: Ownership) -> Option<Self> {
        let inner = NonNull::new(raw)?;
        // Don't free this object when we are dropped.
        if ownership == Ownership::Fontconfig {
            unsafe {
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcPatternReference,
                    inner.as_ptr()
                );
            }
        }
        Some(Self { inner })
    }

    fn add_string(&mut self, object: &CStr, s: &CStr) -> bool {
        // All objects passed to FcPatternAddWhatever are cloned.
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternAddString,
                self.inner.as_ptr(),
                object.as_ptr(),
                s.as_ptr() as *const _
            ) != 0
        }
    }

    fn add_bool(&mut self, object: &CStr, b: bool) -> bool {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternAddBool,
                self.inner.as_ptr(),
                object.as_ptr(),
                b as i32
            ) != 0
        }
    }

    fn default_substitute(&mut self) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcDefaultSubstitute,
                self.inner.as_ptr()
            );
        }
    }

    fn add_charset(&mut self, object: &CStr, s: &CharSet) -> bool {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternAddCharSet,
                self.inner.as_ptr(),
                object.as_ptr(),
                s.inner.as_ptr()
            ) != 0
        }
    }

    fn add_langset(&mut self, object: &CStr, s: &LangSet) -> bool {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternAddLangSet,
                self.inner.as_ptr(),
                object.as_ptr(),
                s.inner.as_ptr()
            ) != 0
        }
    }

    fn get_string<'a>(&'a self, object: &CStr, n: u32) -> MatchResult<Cow<'a, str>> {
        Ok(self.get_c_string(object, n)?.to_string_lossy())
    }

    fn get_c_string<'a>(&'a self, object: &CStr, n: u32) -> MatchResult<&'a CStr> {
        let mut dest: *mut FcChar8 = std::ptr::null_mut();
        let result = unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternGetString,
                self.inner.as_ptr(),
                object.as_ptr(),
                n.try_into().map_err(|_| MatchErr::Other)?,
                &raw mut dest
            )
        };
        if result != FcResultMatch {
            return Err(MatchErr::from_raw(result));
        }
        let dest = NonNull::new(dest).ok_or(MatchErr::Other)?;
        Ok(unsafe { CStr::from_ptr(dest.as_ptr() as *const _) })
    }

    fn get_int(&self, object: &CStr, n: u32) -> MatchResult<i32> {
        let mut dest = 0;
        let result = unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternGetInteger,
                self.inner.as_ptr(),
                object.as_ptr(),
                n.try_into().map_err(|_| MatchErr::Other)?,
                &raw mut dest
            )
        };
        if result != FcResultMatch {
            return Err(MatchErr::from_raw(result));
        }
        Ok(dest)
    }
}

impl Clone for Pattern {
    fn clone(&self) -> Self {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternReference,
                self.inner.as_ptr()
            );
        };
        Self { inner: self.inner }
    }
}

impl Drop for Pattern {
    fn drop(&mut self) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcPatternDestroy,
                self.inner.as_ptr()
            );
        };
    }
}

impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match NonNull::new(unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcNameUnparse,
                self.inner.as_ptr()
            )
        }) {
            Some(unparsed) => {
                let printable = unsafe {
                    &CStr::from_ptr(unparsed.as_ptr() as *const c_char).to_string_lossy()
                };
                let res = f.write_str(printable);
                unsafe {
                    ffi_dispatch!(
                        feature = "fontconfig-dlopen",
                        LIB,
                        FcStrFree,
                        unparsed.as_ptr()
                    );
                };
                res
            }
            None => f.debug_struct("Pattern").finish(),
        }
    }
}

struct FontSet<'a> {
    inner: NonNull<FcFontSet>,
    ownership: Ownership,
    // If an `FcFontSet` is created from an `FcPattern`, it will reference that
    // pattern's data. Well, maybe. The docs say "The returned FcFontSet
    // references FcPattern structures which may be shared by the return value
    // from multiple FcFontSort calls, applications cannot modify these
    // patterns." It's unclear whether this refers to actual lifetime/ownership
    // semantics or if everything's properly refcounted and you're just not
    // allowed to mutate them.
    _parent: PhantomData<&'a ()>,
}

impl FontSet<'_> {
    unsafe fn from_raw(raw: *mut FcFontSet, ownership: Ownership) -> Option<Self> {
        let inner = NonNull::new(raw)?;
        Some(Self {
            inner,
            ownership,
            _parent: PhantomData,
        })
    }

    fn iter(&self) -> FontSetIter<'_> {
        FontSetIter {
            i: 0,
            font_set: self,
        }
    }
}

impl Drop for FontSet<'_> {
    fn drop(&mut self) {
        if self.ownership == Ownership::Application {
            unsafe {
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcFontSetDestroy,
                    self.inner.as_ptr()
                );
            };
        }
    }
}

struct FontSetIter<'a> {
    i: usize,
    font_set: &'a FontSet<'a>,
}

impl Iterator for FontSetIter<'_> {
    type Item = Pattern;

    fn next(&mut self) -> Option<Self::Item> {
        let font_set = self.font_set.inner.as_ptr();
        if self.i >= unsafe { (*font_set).nfont }.try_into().ok()? {
            None
        } else {
            let pattern: *mut FcPattern = unsafe { *(*font_set).fonts.add(self.i) };
            self.i += 1;
            Some(unsafe { Pattern::from_raw(pattern, Ownership::Fontconfig) }?)
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let nfont: Result<usize, _> = unsafe { (*self.font_set.inner.as_ptr()).nfont }.try_into();
        let Ok(nfont) = nfont else {
            return (0, None);
        };
        (nfont, Some(nfont))
    }
}

struct LangSet {
    inner: NonNull<FcLangSet>,
}

impl LangSet {
    fn new() -> Option<Self> {
        let inner = NonNull::new(unsafe {
            ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcLangSetCreate,)
        })?;
        Some(Self { inner })
    }

    fn add(&mut self, lang: &CStr) -> bool {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcLangSetAdd,
                self.inner.as_ptr(),
                lang.as_ptr() as *const _
            ) != 0
        }
    }
}

impl Drop for LangSet {
    fn drop(&mut self) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcLangSetDestroy,
                self.inner.as_ptr()
            );
        };
    }
}

impl Clone for LangSet {
    fn clone(&self) -> Self {
        Self {
            inner: unsafe {
                NonNull::new(ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcLangSetCopy,
                    self.inner.as_ptr()
                ))
                .unwrap()
            },
        }
    }
}

struct CharSet {
    inner: NonNull<FcCharSet>,
}

impl CharSet {
    fn new() -> Option<Self> {
        let inner = NonNull::new(unsafe {
            ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcCharSetCreate,)
        })?;
        Some(Self { inner })
    }

    fn add(&mut self, c: char) -> bool {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcCharSetAddChar,
                self.inner.as_ptr(),
                c as u32
            ) != 0
        }
    }
}

impl Drop for CharSet {
    fn drop(&mut self) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcCharSetDestroy,
                self.inner.as_ptr()
            );
        };
    }
}

impl Clone for CharSet {
    fn clone(&self) -> Self {
        Self {
            inner: unsafe {
                NonNull::new(ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcCharSetCopy,
                    self.inner.as_ptr()
                ))
                .unwrap()
            },
        }
    }
}

struct Config {
    inner: NonNull<FcConfig>,
}

impl Config {
    unsafe fn from_raw(raw: *mut FcConfig, ownership: Ownership) -> Option<Self> {
        let inner = NonNull::new(raw)?;
        // Don't free this object when we are dropped.
        if ownership == Ownership::Fontconfig {
            unsafe {
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcConfigReference,
                    inner.as_ptr()
                );
            }
        }
        Some(Self { inner })
    }

    fn substitute(&self, pattern: &mut Pattern, kind: FcMatchKind) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcConfigSubstitute,
                self.inner.as_ptr(),
                pattern.inner.as_ptr(),
                kind
            )
        };
    }

    fn font_sort<'me, 'ret, 'pat: 'ret>(
        &'me self,
        pattern: &'pat Pattern,
        trim: bool,
    ) -> MatchResult<FontSet<'ret>> {
        let mut result = 0;
        // The returned FcFontSet is for us to free.
        let font_set = unsafe {
            FontSet::from_raw(
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcFontSort,
                    self.inner.as_ptr(),
                    pattern.inner.as_ptr(),
                    trim as i32,
                    std::ptr::null_mut(),
                    &raw mut result
                ),
                Ownership::Application,
            )
        }
        .ok_or(MatchErr::Other)?;
        if result != FcResultMatch {
            return Err(MatchErr::from_raw(result));
        }

        Ok(font_set)
    }

    fn font_match(&self, pattern: &Pattern) -> MatchResult<Pattern> {
        let mut result = 0;
        let pattern = unsafe {
            Pattern::from_raw(
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcFontMatch,
                    self.inner.as_ptr(),
                    pattern.inner.as_ptr(),
                    &raw mut result
                ),
                Ownership::Application,
            )
        }
        .ok_or(MatchErr::Other)?;
        if result != FcResultMatch {
            return Err(MatchErr::from_raw(result));
        }

        Ok(pattern)
    }

    fn font_render_prepare(&self, pat: &Pattern, font: &Pattern) -> Option<Pattern> {
        unsafe {
            Pattern::from_raw(
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcFontRenderPrepare,
                    self.inner.as_ptr(),
                    pat.inner.as_ptr(),
                    font.inner.as_ptr()
                ),
                Ownership::Application,
            )
        }
    }

    /// The files and directories this configuration read, and the font
    /// directories it scanned.
    fn paths(&self) -> crate::SystemFontPaths {
        let config = self.inner.as_ptr();
        // SAFETY: each `FcConfigGet*` returns a new string list over
        // `config`, which outlives it; `paths_of` frees it.
        unsafe {
            crate::SystemFontPaths {
                config_files: paths_of(ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcConfigGetConfigFiles,
                    config
                )),
                config_dirs: paths_of(ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcConfigGetConfigDirs,
                    config
                )),
                font_dirs: paths_of(ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcConfigGetFontDirs,
                    config
                )),
            }
        }
    }
}

/// The paths in a fontconfig string list, copied out, and the list freed.
///
/// # Safety
///
/// `list` is null or a string list the caller owns (one an `FcConfigGet*`
/// call returned), not used after this call.
unsafe fn paths_of(list: *mut FcStrList) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if list.is_null() {
        return paths;
    }
    loop {
        // SAFETY: `list` is a live list (above). The string it returns
        // belongs to the list and stays valid until the next call; it is
        // copied before that.
        let next =
            unsafe { ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcStrListNext, list) };
        if next.is_null() {
            break;
        }
        // SAFETY: fontconfig's strings are NUL-terminated.
        let bytes = unsafe { CStr::from_ptr(next.cast::<c_char>()) }.to_bytes();
        paths.push(PathBuf::from(OsStr::from_bytes(bytes)));
    }
    // SAFETY: the list is ours, and nothing from it is used after this.
    unsafe { ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcStrListDone, list) };
    paths
}

impl Clone for Config {
    fn clone(&self) -> Self {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcConfigReference,
                self.inner.as_ptr()
            )
        };
        Self { inner: self.inner }
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcConfigDestroy,
                self.inner.as_ptr()
            );
        };
    }
}

/// Cache wrapper that maps Unicode scripts to fontconfig [`CharSet`]s.
#[derive(Default)]
struct ScriptCharSetMap(HashMap<Script, Option<CharSet>>);

impl ScriptCharSetMap {
    fn charset_for_script(&mut self, script: Script) -> Option<&CharSet> {
        match self.0.entry(script) {
            Entry::Occupied(e) => e.into_mut().as_ref(),
            Entry::Vacant(e) => {
                let Some(sample) = script.sample() else {
                    return e.insert(None).as_ref();
                };

                let mut charset = CharSet::new()?;
                for c in sample.chars() {
                    charset.add(c);
                }
                e.insert(Some(charset)).as_ref()
            }
        }
    }
}

/// Raw access to the collection of local system fonts.
#[derive(Default)]
pub(crate) struct SystemFonts {
    pub(crate) name_map: Arc<FamilyNameMap>,
    pub(crate) generic_families: Arc<GenericFamilyMap>,
    source_cache: SourcePathMap,
    family_map: HashMap<FamilyId, Option<FamilyInfo>>,
    config: Option<Config>,
    script_charsets: ScriptCharSetMap,
}

unsafe impl Send for SystemFonts {}

impl SystemFonts {
    pub(crate) fn new() -> Self {
        // We couldn't find the fontconfig library; maybe it doesn't exist. Just
        // return a `SystemFonts` with no `config`. All our methods will return
        // `None` and shouldn't attempt any FFI calls because the first thing we
        // do is check for `config`.
        #[cfg(feature = "fontconfig-dlopen")]
        if LIB_RESULT.is_err() {
            return Self::default();
        }

        // Initialize the config
        let config =
            unsafe { ffi_dispatch!(feature = "fontconfig-dlopen", LIB, FcInitLoadConfig,) };
        // fontconfig returns a new config object each time we call FcInitLoadConfig
        let Some(config) = (unsafe { Config::from_raw(config, Ownership::Application) }) else {
            return Self::default();
        };
        unsafe {
            ffi_dispatch!(
                feature = "fontconfig-dlopen",
                LIB,
                FcConfigBuildFonts,
                config.inner.as_ptr()
            );
        }

        // Get all the fonts.

        // The fontconfig docs say this "isn't threadsafe", but this seems to be
        // related to refcounting:
        // https://gitlab.freedesktop.org/fontconfig/fontconfig/-/commit/b5bcf61fe789e66df2de609ec246cb7e4d326180
        // The source code for this function just calls `FcConfigGetCurrent` if
        // none is provided (which we do, and it's atomic anyway) and then
        // dereferences a pointer. I *think* the safety issue they're referring
        // to is that if we destroyed this config on another thread and then
        // tried to access what it returns, it would dereference a null pointer.
        // But we're not doing that.
        let Some(font_set) = (unsafe {
            FontSet::from_raw(
                ffi_dispatch!(
                    feature = "fontconfig-dlopen",
                    LIB,
                    FcConfigGetFonts,
                    config.inner.as_ptr(),
                    FcSetSystem
                ),
                Ownership::Fontconfig,
            )
        }) else {
            return Self::default();
        };

        // Populate the family name map.
        //
        // Every family name a font lists is a family of its own, the way
        // fontconfig matches a family: a font is in family `X` if `X` is any
        // of its `FC_FAMILY` names (`family_uncached` collects them). A second
        // name used to be an alias of the font's first name, which sent a
        // name to the wrong family: `/usr/local/share/fonts/a/ariblk.ttf`
        // lists "Arial, Arial Black", so `'Arial Black'` resolved to Arial
        // (Regular), or to nothing when another font's first name was
        // "Arial Black" (rinch #2294). A second name that is an RBIZ name
        // ("Roboto Light") is now the faces that list it, as
        // fontconfig matches it.
        let mut name_map = FamilyNameMap::default();
        for pattern in font_set.iter() {
            let mut i = 0;
            while let Ok(name) = pattern.get_string(FC_FAMILY, i) {
                name_map.get_or_insert(&name);
                i += 1;
            }
        }

        // Populate the generic family map.
        let mut generic_families = GenericFamilyMap::default();
        for (generic_family, name) in GENERIC_FAMILY_NAMES.iter().copied() {
            let mut pattern = Pattern::new().unwrap();
            pattern.add_string(FC_FAMILY, name);
            // TODO: do we need FcConfigSetDefaultSubstitute?

            config.substitute(&mut pattern, FcMatchPattern);

            // We enable the "trim" option here which ignores later fonts if
            // they provide no new Unicode coverage. On error this generic
            // family simply has no members; in particular `font_sort` errors
            // with `NoMatch` when there are no fonts to sort at all (e.g. a
            // fontconfig configuration exposing an empty system font set).
            let Ok(font_set) = config.font_sort(&pattern, true) else {
                continue;
            };

            // There are a lot of duplicate font families in the substituted
            // pattern. Keep track of which ones have already been added to the
            // list.
            let mut added_families = HashSet::new();

            generic_families.append(
                generic_family,
                font_set.iter().filter_map(|font| {
                    // Not sure if FcFontRenderPrepare performs any substitutions
                    // relevant to fallback family name matching, but it's a good
                    // idea to call it just in case.
                    let font = config.font_render_prepare(&pattern, &font)?;
                    // Generic families can have more than one name, but the only
                    // one we care about is the first one.
                    let name = font.get_string(FC_FAMILY, 0).ok()?;
                    let family_name = name_map.get(name.as_ref())?;

                    if !added_families.insert(family_name.id()) {
                        return None;
                    }

                    Some(family_name.id())
                }),
            );
        }

        Self {
            name_map: Arc::new(name_map),
            generic_families: Arc::new(generic_families),
            source_cache: SourcePathMap::default(),
            family_map: HashMap::default(),
            config: Some(config),
            script_charsets: ScriptCharSetMap::default(),
        }
    }

    pub(crate) fn family(&mut self, id: FamilyId) -> Option<FamilyInfo> {
        match self.family_map.get(&id) {
            Some(Some(family)) => return Some(family.clone()),
            Some(None) => return None,
            None => {}
        }

        let family = self.family_uncached(id);
        self.family_map.insert(id, family.clone());
        family
    }

    pub(crate) fn fallback(&mut self, key: impl Into<FallbackKey>) -> Option<FamilyId> {
        let config = self.config.as_ref()?;
        let key: FallbackKey = key.into();

        let mut pattern = Pattern::new()?;

        let locale_lang_set = key.locale_str().and_then(|locale| {
            let mut lang_set = LangSet::new()?;
            lang_set.add(CString::new(locale).ok()?.as_c_str());
            Some(lang_set)
        });
        let script_char_set = self.script_charsets.charset_for_script(key.script());

        if let Some(set) = locale_lang_set {
            pattern.add_langset(FC_LANG, &set);
        }
        if let Some(set) = script_char_set {
            pattern.add_charset(FC_CHARSET, set);
        }

        config.substitute(&mut pattern, FcMatchPattern);

        // This calls FcFontRenderPrepare for us.
        let font = config.font_match(&pattern).ok()?;

        let family_name = font.get_string(FC_FAMILY, 0).ok()?;
        self.name_map.get(&family_name).map(FamilyName::id)
    }
}

impl SystemFonts {
    /// The files and directories the scan's configuration read, and the font
    /// directories it scanned; empty without fontconfig.
    pub(crate) fn paths(&self) -> crate::SystemFontPaths {
        self.config.as_ref().map(Config::paths).unwrap_or_default()
    }

    /// fontconfig's answer for the family `name`, asked as Chrome's Linux
    /// font manager asks it (Skia's `SkFontConfigInterfaceDirect::
    /// matchFamilyName`): a scalable, SFNT-wrapped pattern for `name`, with
    /// the configuration's and the default substitutions applied. Its first
    /// family after substitution, and the family names of `FcFontMatch`'s
    /// best font.
    pub(crate) fn family_match(&mut self, name: &str) -> Option<crate::SystemFamilyMatch> {
        let config = self.config.as_ref()?;
        let mut pattern = Pattern::new()?;
        pattern.add_string(FC_FAMILY, CString::new(name).ok()?.as_c_str());
        pattern.add_bool(FC_SCALABLE, true);
        // Skia adds this too, so a fontconfig that knows it prefers SFNT
        // fonts; one that does not ignores an unknown object.
        pattern.add_string(c"fontwrapper", c"SFNT");
        config.substitute(&mut pattern, FcMatchPattern);
        pattern.default_substitute();
        let configured = pattern.get_string(FC_FAMILY, 0).ok()?.into_owned();
        let font = config.font_match(&pattern).ok()?;
        let matched = (0..)
            .map_while(|i| font.get_string(FC_FAMILY, i).ok().map(Cow::into_owned))
            .collect();
        Some(crate::SystemFamilyMatch {
            configured,
            matched,
        })
    }

    fn family_uncached(&mut self, id: FamilyId) -> Option<FamilyInfo> {
        let config = self.config.as_ref()?;
        let name = self.name_map.get_by_id(id).cloned()?;

        // Match by family name.
        let mut pattern = Pattern::new()?;
        pattern.add_string(FC_FAMILY, CString::new(name.name()).ok()?.as_c_str());
        config.substitute(&mut pattern, FcMatchPattern);

        let fc_fonts = config.font_sort(&pattern, false).ok()?;
        let mut font_infos = SmallVec::<[FontInfo; 4]>::new();
        for font in fc_fonts.iter() {
            let Some(font) = config.font_render_prepare(&pattern, &font) else {
                continue;
            };
            // We've performed font substitution and then sorted everything by
            // "closeness", so the good fonts should be at the top. Once we see
            // a fallback font (one that's not part of the family we explicitly
            // asked for), we can stop. A font is part of the family when any
            // of its family names is the one asked for, not only its first:
            // fontconfig sorts by all of them (rinch #2294).
            let mut family_names = (0..).map_while(|i| font.get_string(FC_FAMILY, i).ok());
            if !family_names.any(|family_name| same_family_name(&family_name, name.name())) {
                break;
            }

            if let Some(font_info) = (|| {
                let path = font.get_c_string(FC_FILE, 0).ok()?;
                // This part is Unix-specific. Sorry, Windows fontconfig user.
                let path = Path::new(OsStr::from_bytes(path.to_bytes()));
                let source_info = self.source_cache.get_or_insert(path);

                let weight = font
                    .get_int(FC_WEIGHT, 0)
                    .map(FontWeight::from_fontconfig)
                    .unwrap_or_default();
                let width = font
                    .get_int(FC_WIDTH, 0)
                    .map(FontWidth::from_fontconfig)
                    .unwrap_or_default();
                let style = font
                    .get_int(FC_SLANT, 0)
                    .map(FontStyle::from_fontconfig)
                    .unwrap_or_default();
                let index = font.get_int(FC_INDEX, 0).map_or(0, |idx| idx.max(0) as u32);

                let mut font_info = FontInfo::from_source(source_info, index)?;
                // TODO(valadaptive): does this do anything anymore?
                font_info.maybe_override_attributes(width, style, weight);
                Some(font_info)
            })() {
                font_infos.push(font_info);
            }
        }

        if font_infos.is_empty() {
            return None;
        }

        Some(FamilyInfo::new(name, font_infos))
    }
}

const GENERIC_FAMILY_NAMES: &[(GenericFamily, &CStr)] = &[
    (GenericFamily::Serif, c"serif"),
    (GenericFamily::SansSerif, c"sans-serif"),
    (GenericFamily::Monospace, c"monospace"),
    (GenericFamily::Cursive, c"cursive"),
    (GenericFamily::Fantasy, c"fantasy"),
    (GenericFamily::SystemUi, c"system-ui"),
    (GenericFamily::Emoji, c"emoji"),
    (GenericFamily::Math, c"math"),
];

/// Whether two family names are the same name, compared as the family name
/// map compares them (case-insensitively).
fn same_family_name(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}
