// Builds every deliverable for each icon direction:
//   <out>/<variant>/svg/   master + optical sizes
//   <out>/<variant>/png/   every Windows/web size, optical artwork used at <= 32 px
//   <out>/<variant>/tauri/ drop-in replacement for src-tauri/icons (ico, icns, pngs, store logos)
// usage: node export.js <out-dir>
const { Resvg } = require("@resvg/resvg-js");
const fs = require("fs");
const path = require("path");
const V = require("./variants");
const S = require("./small-art");

const OUT = path.resolve(process.argv[2] || path.join(__dirname, ".."));
const NAMES = { maske: "a-maske", versteck: "b-versteck", schweif: "c-schweif", siegel: "d-siegel" };
const OPTICAL = [16, 20, 24, 32];

// The optical artwork exists for 16/20/24/32. A size in between (30) uses the next
// larger optical drawing, scaled down slightly; above 32 the master takes over.
function svgFor(n, size) {
  if (size <= 32) return S[n + "_small"](OPTICAL.find((o) => o >= size));
  return V[n]();
}
const png = (n, size) => Buffer.from(new Resvg(svgFor(n, size), { fitTo: { mode: "width", value: size } }).render().asPng());
const rgbaPremul = (n, size) => new Resvg(svgFor(n, size), { fitTo: { mode: "width", value: size } }).render();

// ICO: 32-bit DIB for < 256 (best compatibility with rc.exe and old shells), PNG for 256.
function ico(n, sizes) {
  const imgs = sizes.map((s) => {
    if (s >= 256) return { s, data: png(n, s) };
    const r = rgbaPremul(n, s), px = r.pixels, w = s, h = s;
    const header = Buffer.alloc(40);
    header.writeUInt32LE(40, 0); header.writeInt32LE(w, 4); header.writeInt32LE(h * 2, 8);
    header.writeUInt16LE(1, 12); header.writeUInt16LE(32, 14);
    const xor = Buffer.alloc(w * h * 4);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4, o = ((h - 1 - y) * w + x) * 4, a = px[i + 3];
      const un = (c) => (a ? Math.min(255, Math.round((c * 255) / a)) : 0); // un-premultiply
      xor[o] = un(px[i + 2]); xor[o + 1] = un(px[i + 1]); xor[o + 2] = un(px[i]); xor[o + 3] = a;
    }
    const rowBytes = Math.ceil(w / 32) * 4, and = Buffer.alloc(rowBytes * h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++)
      if (px[(y * w + x) * 4 + 3] === 0) and[(h - 1 - y) * rowBytes + (x >> 3)] |= 0x80 >> (x & 7);
    return { s, data: Buffer.concat([header, xor, and]) };
  });
  const dir = Buffer.alloc(6 + 16 * imgs.length);
  dir.writeUInt16LE(0, 0); dir.writeUInt16LE(1, 2); dir.writeUInt16LE(imgs.length, 4);
  let off = dir.length;
  imgs.forEach(({ s, data }, i) => {
    const e = 6 + 16 * i;
    dir.writeUInt8(s >= 256 ? 0 : s, e); dir.writeUInt8(s >= 256 ? 0 : s, e + 1);
    dir.writeUInt16LE(1, e + 4); dir.writeUInt16LE(32, e + 6);
    dir.writeUInt32LE(data.length, e + 8); dir.writeUInt32LE(off, e + 12);
    off += data.length;
  });
  return Buffer.concat([dir, ...imgs.map((i) => i.data)]);
}

// ICNS with PNG payloads
function icns(n) {
  const types = [["icp4", 16], ["icp5", 32], ["icp6", 64], ["ic07", 128], ["ic08", 256], ["ic09", 512], ["ic10", 1024], ["ic11", 32], ["ic12", 64], ["ic13", 256], ["ic14", 512]];
  const parts = types.map(([t, s]) => {
    const d = png(n, s), h = Buffer.alloc(8);
    h.write(t, 0, "ascii"); h.writeUInt32BE(d.length + 8, 4);
    return Buffer.concat([h, d]);
  });
  const body = Buffer.concat(parts), h = Buffer.alloc(8);
  h.write("icns", 0, "ascii"); h.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([h, body]);
}

const ICO_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256];
const PNG_SIZES = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128, 256, 512, 1024];
const TAURI = { "32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512,
  "Square30x30Logo.png": 30, "Square44x44Logo.png": 44, "Square71x71Logo.png": 71, "Square89x89Logo.png": 89,
  "Square107x107Logo.png": 107, "Square142x142Logo.png": 142, "Square150x150Logo.png": 150,
  "Square284x284Logo.png": 284, "Square310x310Logo.png": 310, "StoreLogo.png": 50 };

for (const [n, dir] of Object.entries(NAMES)) {
  const base = path.join(OUT, dir);
  for (const sub of ["svg", "png", "tauri"]) fs.mkdirSync(path.join(base, sub), { recursive: true });
  fs.writeFileSync(path.join(base, "svg", "icon.svg"), V[n]());
  for (const o of OPTICAL) fs.writeFileSync(path.join(base, "svg", `icon-${o}.svg`), S[n + "_small"](o));
  for (const s of PNG_SIZES) fs.writeFileSync(path.join(base, "png", `${s}.png`), png(n, s));
  for (const [f, s] of Object.entries(TAURI)) fs.writeFileSync(path.join(base, "tauri", f), png(n, s));
  fs.writeFileSync(path.join(base, "tauri", "icon.ico"), ico(n, ICO_SIZES));
  fs.writeFileSync(path.join(base, "tauri", "icon.icns"), icns(n));
  fs.writeFileSync(path.join(base, "tauri", "icon.svg"), V[n]());
  console.log(dir, "ico", fs.statSync(path.join(base, "tauri", "icon.ico")).size, "icns", fs.statSync(path.join(base, "tauri", "icon.icns")).size);
}
