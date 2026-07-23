# Plot

**Resource pack repair tool for Minecraft 1.8 PvP (PotPvP)**

[中文](README.md)

## What it is

Minecraft 1.8 only loads resource packs whose **zip root** contains `assets` and `pack.mcmeta` directly. Community packs often nest an extra folder/zip, use the wrong extension, mistype core filenames, or ship a `pack.mcmeta` with **illegal JSON escapes** that Lunar Client rejects. The game usually fails **silently**.

Plot scans your `resourcepacks` folder, classifies each top-level entry by what MC 1.8 can actually read, and rebuilds fixable packs into clean standard zips.

- **Windows only** · single-file `plot.exe` · [Apache-2.0](LICENSE)  
- Requires WebView2 (usually present on Windows 10/11)

## Features

| Group | Meaning (short) |
|-------|-----------------|
| **Illegal** (red) | Unreadable / non-zip magic / real RAR·7z, etc. → `plot_temp/illegal_packs` |
| **Nested** (orange) | Wrappers, bad extension, `pack.mcmeta` case/typos → unwrap & rebuild |
| **Illegal in LC** (ice cyan) | Lunar-illegal escapes in `pack.mcmeta` → minimal fix, keep colors/text |
| **Folder** (yellow) | Valid folder pack → zip it |
| **Bloated** (purple) | Readable but noisy root / dead paths → slim rebuild |
| **Normal** (green) | Target shape; left alone when processing |

Also:

- **Collections** split into separate packs; nested products use the **inner author name**  
- **No data loss**: originals move to `plot_temp/problematic_packs` beside the exe  
- **No overwrite**: colliding names get ` (1)`, ` (2)`, …  
- **Lock preflight**: packs held by a running MC are skipped, not force-edited  
- **Updates**: silent GitHub Release check on startup; footer button for manual check (update / latest / failed)  
- **UI**: Simplified Chinese / Traditional Chinese / English  

## Usage

1. Run `plot.exe` (defaults to `%APPDATA%\.minecraft\resourcepacks`)  
2. Review the six-group overview and pack list  
3. **Process** → confirm the checklist (including locked packs) → go  
4. Check the result dialog; delete `plot_temp` only after you are sure  

## v1 limitations

- **Zip only**. RAR/7z are marked illegal with a “extract and rescan” hint  
- Windows only  

## Development

```bash
npm install
npm run tauri dev
npm run test
cargo test
npm run tauri build    # → target/release/plot.exe
```

| Path | Role |
|------|------|
| `engine/` | Pure Rust domain (scan / classify / process) |
| `src-tauri/` | Thin Tauri commands + WebView2 guard |
| `src/` | React UI; `ipc.ts` is the only invoke boundary |

Domain glossary: [`CONTEXT.md`](CONTEXT.md).

## License

[Apache License 2.0](LICENSE)
