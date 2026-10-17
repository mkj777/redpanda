# redpanda app icon

Four directions, each a complete, drop-in icon set:

| Folder | Direction |
| --- | --- |
| `a-maske/` | Only the white face markings on a rust plate; the white muzzle is an open envelope |
| `b-versteck/` | Fluent style without a plate: a red panda peeking out of an envelope (in use) |
| `c-schweif/` | The @ drawn as a ringed red panda tail on a dark plate |
| `d-siegel/` | Paper plate as the letter, wax seal with the mask as emblem |

Each folder contains:

- `svg/icon.svg`: 1024 master; `svg/icon-16.svg` to `icon-32.svg`: optical sizes drawn on the pixel grid
- `png/`: 16 to 1024 px (optical artwork at 32 px and below)
- `tauri/`: same file names as `src-tauri/icons`, including a 14-size `icon.ico` (32-bit DIB below 256, PNG at 256) and `icon.icns`

The app icon is generated from the 1024 px PNG master, downscaled to every size, so the small optical artwork is not used in the app. To switch it, run `pnpm tauri icon design/icon/<folder>/png/1024.png`, delete the generated `android/` and `ios/` folders in `src-tauri/icons`, and copy `svg/icon.svg` to `public/icon.svg`.

## Regenerate

```sh
cd design/icon/tools
npm install
node export.js   # writes the four folders next to tools/
node lab.js      # perception tests and metrics into tools/lab/
```
