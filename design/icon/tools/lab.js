// Test lab: perception tests (grayscale, blur, silhouette, colour vision deficiency)
// and measurements (coverage, centroid, WCAG pairs, edge contrast, small-size crispness).
const { Resvg } = require("@resvg/resvg-js");
const sharp = require("sharp");
const fs = require("fs");
const path = require("path");
const V = require("./variants");
const S = require("./small-art");

const OUT = process.argv[2] || require("path").join(__dirname, "lab");
fs.mkdirSync(OUT, { recursive: true });
const NAMES = ["maske", "versteck", "schweif", "siegel"];
const svgAt = (n, s) => (s <= 32 && S[n + "_small"](s)) || V[n]();
const raster = (svg, s) => new Resvg(svg, { fitTo: { mode: "width", value: s } }).render();
const rgba = (svg, s) => { const r = raster(svg, s); return { w: r.width, h: r.height, d: Buffer.from(r.pixels) }; };

// --- colour helpers
const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const gam = (v) => { v = Math.max(0, Math.min(1, v)); return Math.round(255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055)); };
const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

const CVD = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
};

function over(img, bg) {
  const b = hex(bg), o = Buffer.alloc(img.w * img.h * 3);
  for (let i = 0; i < img.w * img.h; i++) {
    const a = img.d[i * 4 + 3] / 255;
    for (let k = 0; k < 3; k++) o[i * 3 + k] = Math.round(img.d[i * 4 + k] + b[k] * (1 - a)); // resvg output is premultiplied
  }
  return o;
}
const toPng = (buf, w, h, ch = 3) => sharp(buf, { raw: { width: w, height: h, channels: ch } }).png().toBuffer();

(async () => {
  const metrics = {};
  const tiles = [];
  for (const n of NAMES) {
    const m = (metrics[n] = {});
    // coverage + centroid at 256
    const big = rgba(V[n](), 256);
    let cov = 0, cx = 0, cy = 0, minx = 256, miny = 256, maxx = 0, maxy = 0;
    for (let y = 0; y < 256; y++) for (let x = 0; x < 256; x++) {
      const a = big.d[(y * 256 + x) * 4 + 3] / 255;
      cov += a; cx += a * x; cy += a * y;
      if (a > 0.5) { minx = Math.min(minx, x); maxx = Math.max(maxx, x); miny = Math.min(miny, y); maxy = Math.max(maxy, y); }
    }
    m.coverage = +(cov / 65536).toFixed(3);
    m.centroid = [+(cx / cov / 256).toFixed(3), +(cy / cov / 256).toFixed(3)];
    m.bbox = [minx, miny, maxx + 1, maxy + 1].map((v) => +(v / 256).toFixed(3));

    // edge contrast at 24 px against Windows 11 taskbar light (#F3F3F3) and dark (#202020)
    m.edge = {};
    for (const s of [16, 24, 32]) {
      const img = rgba(svgAt(n, s), s);
      for (const [label, bg] of [["light", "#F3F3F3"], ["dark", "#202020"]]) {
        const flat = over(img, bg), B = hex(bg);
        let acc = 0, cnt = 0;
        for (let y = 0; y < s; y++) for (let x = 0; x < s; x++) {
          const i = y * s + x, a = img.d[i * 4 + 3];
          if (a < 128) continue;
          const nb = [[1, 0], [-1, 0], [0, 1], [0, -1]].some(([dx, dy]) => {
            const X = x + dx, Y = y + dy;
            return X < 0 || Y < 0 || X >= s || Y >= s || img.d[(Y * s + X) * 4 + 3] < 128;
          });
          if (!nb) continue;
          acc += ratio([flat[i * 3], flat[i * 3 + 1], flat[i * 3 + 2]], B); cnt++;
        }
        m.edge[`${s}_${label}`] = +(acc / cnt).toFixed(2);
      }
    }

    // crispness: mean gradient magnitude, tuned vs plain downscale of the master
    m.crisp = {};
    for (const s of [16, 24, 32]) {
      const g = (svg) => {
        const img = rgba(svg, s), f = over(img, "#808080");
        let sum = 0;
        for (let y = 0; y < s - 1; y++) for (let x = 0; x < s - 1; x++) {
          const L = (i) => lum([f[i * 3], f[i * 3 + 1], f[i * 3 + 2]]);
          const i = y * s + x;
          sum += Math.abs(L(i + 1) - L(i)) + Math.abs(L(i + s) - L(i));
        }
        return sum / (s * s);
      };
      const t = g(svgAt(n, s)), p = g(V[n]());
      m.crisp[s] = { tuned: +t.toFixed(4), master: +p.toFixed(4), gain: +((t / p - 1) * 100).toFixed(0) };
    }

    // perception tiles at 96 px: normal, gray, blur, silhouette, protan, deutan, tritan
    const s = 96, img = rgba(V[n](), s), flat = over(img, "#9A9A9A");
    const row = [];
    row.push(await toPng(flat, s, s));
    const gray = Buffer.alloc(s * s * 3);
    for (let i = 0; i < s * s; i++) { const v = gam(lum([flat[i * 3], flat[i * 3 + 1], flat[i * 3 + 2]])); gray.fill(v, i * 3, i * 3 + 3); }
    row.push(await toPng(gray, s, s));
    row.push(await sharp(await toPng(flat, s, s)).blur(3.2).png().toBuffer());
    const sil = Buffer.alloc(s * s * 3);
    for (let i = 0; i < s * s; i++) { const a = img.d[i * 4 + 3] / 255; sil.fill(Math.round(154 * (1 - a) + 20 * a), i * 3, i * 3 + 3); }
    row.push(await toPng(sil, s, s));
    for (const M of Object.values(CVD)) {
      const o = Buffer.alloc(s * s * 3);
      for (let i = 0; i < s * s; i++) {
        const c = [0, 1, 2].map((k) => lin(flat[i * 3 + k]));
        for (let k = 0; k < 3; k++) o[i * 3 + k] = gam(M[k][0] * c[0] + M[k][1] * c[1] + M[k][2] * c[2]);
      }
      row.push(await toPng(o, s, s));
    }
    tiles.push(row);
    for (let k = 0; k < row.length; k++) fs.writeFileSync(path.join(OUT, `${n}-test-${["normal", "gray", "blur", "silhouette", "protan", "deutan", "tritan"][k]}.png`), row[k]);
  }

  // WCAG pairs on the key colour relationships
  const pairs = {
    maske: [["Creme / Rost-Plate (Mitte)", "#FFF8EF", "#D9581E"], ["Auge / Rost-Plate", "#1C1917", "#D9581E"], ["Nase / Creme", "#1C1917", "#FFF8EF"]],
    versteck: [["Kopf / Tasche", "#D9581E", "#FFFFFF"], ["Auge / Kopf", "#1C1917", "#D9581E"], ["Tasche / Rückseite", "#FFFFFF", "#D8B694"]],
    schweif: [["Rost / Plate", "#D9581E", "#231E1B"], ["Ring / Rost", "#F7D9B6", "#D9581E"], ["Ring / Plate", "#F7D9B6", "#231E1B"]],
    siegel: [["Siegel / Papier", "#C9461A", "#F6EBDD"], ["Emblem-Creme / Siegel", "#FFF8EF", "#C9461A"], ["Papier / Taskleiste hell", "#F6EBDD", "#F3F3F3"]],
  };
  for (const n of NAMES) metrics[n].wcag = pairs[n].map(([l, a, b]) => ({ label: l, a, b, ratio: +ratio(hex(a), hex(b)).toFixed(2) }));

  fs.writeFileSync(path.join(OUT, "metrics.json"), JSON.stringify(metrics, null, 2));
  console.log(JSON.stringify(metrics, null, 1));
})();
