//! Nerd Font icons for files and folders: by exact name first, then by extension.

pub fn icon(name: &str, dir: bool, link: bool, exec: bool, open: bool) -> char {
    let lower = name.to_lowercase();
    if dir {
        return folder(&lower).unwrap_or(match (link, open) {
            (true, _) => '\u{f482}',
            (_, true) => '\u{f07c}',
            _ => '\u{f07b}',
        });
    }
    if let Some(c) = named(&lower) {
        return c;
    }
    let ext = lower.rsplit_once('.').map_or("", |x| x.1);
    extension(ext).unwrap_or(if link {
        '\u{f481}'
    } else if exec {
        '\u{f489}'
    } else {
        '\u{f15b}'
    })
}

fn folder(name: &str) -> Option<char> {
    Some(match name {
        ".git" => '\u{e5fb}',
        ".github" => '\u{e5fd}',
        ".config" | "config" | ".settings" => '\u{e5fc}',
        "node_modules" => '\u{e5fa}',
        "desktop" => '\u{f108}',
        "documents" | "docs" | "doc" => '\u{f02d}',
        "downloads" => '\u{f019}',
        "music" | "audio" | "sounds" => '\u{f001}',
        "pictures" | "images" | "img" | "assets" | "screenshots" | "photos" | "wallpapers" => '\u{f03e}',
        "videos" | "movies" => '\u{f03d}',
        "src" | "source" | "lib" | "crates" => '\u{f121}',
        "bin" | "scripts" | ".local" => '\u{f489}',
        "test" | "tests" | "spec" | "__tests__" => '\u{f0c3}',
        "target" | "build" | "dist" | "out" | "release" | "debug" => '\u{f487}',
        "fonts" | ".fonts" => '\u{f031}',
        ".ssh" | ".gnupg" | "keys" => '\u{f084}',
        ".cargo" | ".rustup" => '\u{e7a8}',
        ".npm" | ".bun" => '\u{e71e}',
        "go" => '\u{e626}',
        ".cache" | "cache" | "tmp" | "temp" => '\u{f1da}',
        "games" | "steam" | ".steam" | "steamapps" => '\u{f11b}',
        "trash" | ".trash" => '\u{f1f8}',
        "logo" | "design" | "art" => '\u{f1fc}',
        "utils" | "tools" => '\u{f0ad}',
        "dotfiles" => '\u{f013}',
        "android" | ".android" => '\u{f17b}',
        "mods" | "plugins" | "extensions" => '\u{f12e}',
        _ => return None,
    })
}

fn named(name: &str) -> Option<char> {
    Some(match name {
        "cargo.toml" | "cargo.lock" | "rustfmt.toml" | "rust-toolchain.toml" => '\u{e7a8}',
        "makefile" | "justfile" | "cmakelists.txt" | "meson.build" => '\u{f0ad}',
        "dockerfile" | "containerfile" | "compose.yml" | "compose.yaml" | "docker-compose.yml" | "docker-compose.yaml" => '\u{f308}',
        "license" | "license.md" | "license.txt" | "copying" | "license-mit" | "license-apache" => '\u{f0e3}',
        "readme" | "readme.md" | "readme.txt" => '\u{f05a}',
        ".gitignore" | ".gitattributes" | ".gitmodules" | ".gitconfig" => '\u{f1d3}',
        "package.json" | "package-lock.json" | ".npmrc" | "bun.lockb" | "bun.lock" => '\u{e71e}',
        "pkgbuild" | ".srcinfo" => '\u{f303}',
        ".zshrc" | ".zprofile" | ".zshenv" | ".bashrc" | ".bash_profile" | ".profile" | ".zsh_history" | ".bash_history" => '\u{f489}',
        "go.mod" | "go.sum" => '\u{e626}',
        "requirements.txt" | "pyproject.toml" | "setup.py" | "pipfile" => '\u{e73c}',
        "tsconfig.json" => '\u{e628}',
        "flake.nix" | "flake.lock" => '\u{f313}',
        "id_rsa" | "id_ed25519" | "id_ecdsa" | "authorized_keys" | "known_hosts" => '\u{f084}',
        ".env" | ".envrc" => '\u{f462}',
        "changelog" | "changelog.md" => '\u{f1da}',
        _ => return None,
    })
}

fn extension(ext: &str) -> Option<char> {
    Some(match ext {
        "rs" => '\u{e7a8}',
        "py" | "pyi" | "pyc" | "ipynb" => '\u{e73c}',
        "sh" | "bash" | "zsh" | "fish" | "nu" => '\u{f489}',
        "md" | "markdown" | "mdx" => '\u{f48a}',
        "txt" | "rst" | "org" | "log" => '\u{f15c}',
        "json" | "jsonc" | "json5" => '\u{e60b}',
        "toml" | "yaml" | "yml" | "ini" | "conf" | "cfg" | "kdl" | "ron" => '\u{e615}',
        "xml" | "plist" => '\u{f121}',
        "js" | "mjs" | "cjs" => '\u{e74e}',
        "ts" | "mts" | "cts" => '\u{e628}',
        "jsx" | "tsx" => '\u{e7ba}',
        "html" | "htm" => '\u{e736}',
        "css" | "scss" | "sass" | "less" => '\u{e749}',
        "go" => '\u{e626}',
        "c" | "h" => '\u{e61e}',
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => '\u{e61d}',
        "java" | "jar" | "class" => '\u{e738}',
        "lua" => '\u{e620}',
        "rb" => '\u{e739}',
        "php" => '\u{e73d}',
        "swift" => '\u{e755}',
        "dart" => '\u{e798}',
        "hs" => '\u{e777}',
        "ex" | "exs" => '\u{e62d}',
        "vim" => '\u{e62b}',
        "nix" => '\u{f313}',
        "sql" | "db" | "sqlite" | "sqlite3" => '\u{f1c0}',
        "csv" | "tsv" | "xls" | "xlsx" | "ods" => '\u{f1c3}',
        "doc" | "docx" | "odt" | "rtf" => '\u{f1c2}',
        "ppt" | "pptx" | "odp" => '\u{f1c4}',
        "pdf" => '\u{f1c1}',
        "epub" | "mobi" => '\u{f02d}',
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "ico" | "heic" | "tiff" => '\u{f1c5}',
        "psd" | "kra" | "xcf" | "aseprite" => '\u{f1fc}',
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" => '\u{f1c8}',
        "mp3" | "flac" | "ogg" | "wav" | "opus" | "m4a" | "aac" => '\u{f1c7}',
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar" | "bz2" | "tgz" => '\u{f1c6}',
        "deb" | "rpm" | "pkg" | "appimage" | "flatpak" => '\u{f187}',
        "iso" | "img" | "qcow2" | "vdi" => '\u{f0a0}',
        "exe" | "msi" | "dll" | "bat" => '\u{f17a}',
        "apk" | "aab" => '\u{f17b}',
        "ttf" | "otf" | "woff" | "woff2" => '\u{f031}',
        "lock" => '\u{f023}',
        "key" | "pem" | "crt" | "cer" | "gpg" | "asc" | "pub" => '\u{f084}',
        "desktop" => '\u{f108}',
        "service" | "timer" | "socket" | "mount" | "target" => '\u{f013}',
        "diff" | "patch" => '\u{f440}',
        "env" => '\u{f462}',
        "blend" | "obj" | "fbx" | "stl" | "glb" | "gltf" | "3mf" => '\u{f1b2}',
        "torrent" => '\u{f019}',
        "bak" | "tmp" | "swp" | "old" => '\u{f1da}',
        "wasm" | "o" | "so" | "a" => '\u{f471}',
        _ => return None,
    })
}
