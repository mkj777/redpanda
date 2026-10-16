// Hand-tuned optical sizes. Each artwork is drawn in its own n x n pixel grid,
// edges sit on whole pixels, detail is removed instead of shrunk.
const { squircle, P } = require("./variants");

const svg = (n, body) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${n}" height="${n}" viewBox="0 0 ${n} ${n}" shape-rendering="geometricPrecision">${body}</svg>`;
const grad = (id, a, b) =>
  `<linearGradient id="${id}" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>`;
const poly = (pts, fill) => `<path d="M${pts.map((p) => p.join(",")).join("L")}Z" fill="${fill}"/>`;
const mx = (n, pts) => pts.map(([x, y]) => [n - x, y]);
const rect = (x, y, w, h, fill, rx = 0) => `<rect x="${x}" y="${y}" width="${w}" height="${h}" rx="${rx}" fill="${fill}"/>`;

// ------------------------------------------------------------------ A: Maske
// per size: ears, eyes, envelope (x0, y0, x1, y1, notch tip y), nose, brows
const A = {
  16: { ear: [[2, 5], [2.5, 1], [6.5, 3.5]], earIn: null, eye: [4, 5, 2, 2], env: [2, 7, 14, 14, 10.5], nose: [7, 9, 2, 2], brow: null, r: 1.5 },
  20: { ear: [[2.5, 6.5], [3, 1.5], [8, 4.5]], earIn: null, eye: [5, 6, 2, 2], env: [3, 9, 17, 17, 13], nose: [9, 11, 2, 2], brow: null, r: 2 },
  24: { ear: [[3, 8], [3.5, 2], [9.5, 5.5]], earIn: [[4.3, 7.5], [4.5, 4.2], [8.2, 6]], eye: [6, 8, 3, 3], env: [3, 11, 21, 20, 15.5], nose: [11, 13, 2, 2], brow: null, r: 2.5 },
  32: { ear: [[4, 10], [4.5, 3], [11.5, 7]], earIn: [[5.5, 9.4], [5.6, 5.4], [10, 7.6]], eye: [8, 11, 3, 3], env: [4, 15, 28, 27, 21], nose: [14, 18, 4, 3], brow: null, r: 3 },
};
function maske_small(n) {
  const k = A[n];
  if (!k) return null;
  const [x0, y0, x1, y1, ty] = k.env, c = n / 2, r = k.r;
  const env = `M${x0} ${y0} L${c} ${ty} L${x1} ${y0} V${y1 - r} Q${x1} ${y1} ${x1 - r} ${y1} H${x0 + r} Q${x0} ${y1} ${x0} ${y1 - r} Z`;
  const [ex, ey, ew, eh] = k.eye, [nx, ny, nw, nh] = k.nose;
  return svg(n, `<defs>${grad("p", P.rustHi, P.rustLo)}${grad("c", "#FFFBF6", P.creamLo)}</defs>
  <path d="${squircle(n / 2, n / 2, n / 2, 5, 360)}" fill="url(#p)"/>
  ${poly(k.ear, P.cream)}${poly(mx(n, k.ear), P.cream)}
  ${k.earIn ? poly(k.earIn, P.fur) + poly(mx(n, k.earIn), P.fur) : ""}
  ${k.brow ? `<rect x="${k.brow[0]}" y="${k.brow[1]}" width="${k.brow[2]}" height="${k.brow[3]}" rx="${k.brow[3] / 2}" fill="${P.cream}"/><rect x="${n - k.brow[0] - k.brow[2]}" y="${k.brow[1]}" width="${k.brow[2]}" height="${k.brow[3]}" rx="${k.brow[3] / 2}" fill="${P.cream}"/>` : ""}
  <path d="${env}" fill="url(#c)"/>
  ${rect(ex, ey, ew, eh, P.ink, ew > 2 ? 0.6 : 0)}${rect(n - ex - ew, ey, ew, eh, P.ink, ew > 2 ? 0.6 : 0)}
  ${rect(nx, ny, nw, nh, P.ink, nh > 2 ? 1 : 0.5)}`);
}

// ------------------------------------------------------------------ B: Versteck
const B = {
  16: { back: [1, 7, 15, 15], head: [4, 3, 12, 12], ear: [[3.5, 6], [3.5, 1], [7, 3.5]], eye: [5, 6, 2, 2], muz: null, nose: [7, 8, 2, 0], pocket: [1, 9, 15, 15, 12], r: 1.5 },
  20: { back: [1, 9, 19, 19], head: [5, 4, 15, 15], ear: [[4.5, 7.5], [4.5, 1.5], [9, 4.5]], eye: [6, 8, 2, 2], muz: null, nose: [9, 10, 2, 2], pocket: [1, 11.5, 19, 19, 15], r: 2 },
  24: { back: [1, 10, 23, 23], head: [6, 5, 18, 18], ear: [[5, 9], [5.5, 1.5], [11, 5.5]], eye: [7, 9, 3, 3], muz: [9, 12.5, 15, 20], nose: [11, 12, 2, 2], pocket: [1, 13.5, 23, 23, 18], r: 2.5 },
  32: { back: [1.5, 13.5, 30.5, 30.5], head: [8, 7, 24, 24], ear: [[7, 12], [7.5, 2.5], [14.5, 7.5]], eye: [10, 12, 3, 3], muz: [12, 16, 20, 26], nose: [14, 16, 4, 3], pocket: [1.5, 18, 30.5, 30.5, 24], r: 3, brow: [9.5, 9.5, 4, 1.5] },
};
function versteck_small(n) {
  const k = B[n];
  if (!k) return null;
  const c = n / 2, r = k.r;
  const [bx0, by0, bx1, by1] = k.back;
  const [hx0, hy0, hx1, hy1] = k.head;
  const [px0, py0, px1, py1, pty] = k.pocket;
  const [ex, ey, ew, eh] = k.eye, [nx, ny, nw, nh] = k.nose;
  const headD = `M${hx0} ${hy1} V${hy0 + (hx1 - hx0) / 2} A${(hx1 - hx0) / 2} ${(hx1 - hx0) / 2.2} 0 0 1 ${hx1} ${hy0 + (hx1 - hx0) / 2} V${hy1} Z`;
  const pocket = `M${px0} ${py0} L${c} ${pty} L${px1} ${py0} V${py1 - r} Q${px1} ${py1} ${px1 - r} ${py1} H${px0 + r} Q${px0} ${py1} ${px0} ${py1 - r} Z`;
  const earIn = n >= 24 ? (() => { const [[a, b], [cx, d], [e, f]] = k.ear; const s = 0.5; const gx = (a + cx + e) / 3, gy = (b + d + f) / 3; return [[a, b], [cx, d], [e, f]].map(([x, y]) => [gx + (x - gx) * s, gy + (y - gy) * s + 0.5]); })() : null;
  return svg(n, `<defs>${grad("bk", "#E2C3A4", "#CFA885")}${grad("hd", P.rustHi, P.rustLo)}${grad("fr", "#FFFFFF", "#F2E2D1")}</defs>
  <rect x="${bx0}" y="${by0}" width="${bx1 - bx0}" height="${by1 - by0}" rx="${r}" fill="url(#bk)" stroke="#9C7350" stroke-width="${n >= 24 ? 1 : 0.8}"/>
  ${poly(k.ear, "url(#hd)")}${poly(mx(n, k.ear), "url(#hd)")}
  ${earIn ? poly(earIn, P.cream) + poly(mx(n, earIn), P.cream) : ""}
  <path d="${headD}" fill="url(#hd)"/>
  ${k.brow ? `<rect x="${k.brow[0]}" y="${k.brow[1]}" width="${k.brow[2]}" height="${k.brow[3]}" rx="0.75" fill="${P.cream}"/><rect x="${n - k.brow[0] - k.brow[2]}" y="${k.brow[1]}" width="${k.brow[2]}" height="${k.brow[3]}" rx="0.75" fill="${P.cream}"/>` : ""}
  ${k.muz ? `<rect x="${k.muz[0]}" y="${k.muz[1]}" width="${k.muz[2] - k.muz[0]}" height="${k.muz[3] - k.muz[1]}" rx="${(k.muz[2] - k.muz[0]) / 2}" fill="${P.cream}"/>` : ""}
  ${rect(ex, ey, ew, eh, P.ink, ew > 2 ? 0.6 : 0)}${rect(n - ex - ew, ey, ew, eh, P.ink, ew > 2 ? 0.6 : 0)}
  ${rect(nx, ny, nw, nh, P.ink, nh > 2 ? 1 : 0.4)}
  <path d="${pocket}" fill="url(#fr)" stroke="#9C7350" stroke-opacity="0.9" stroke-width="${n >= 24 ? 0.75 : 0.6}" stroke-linejoin="round"/>`);
}

// ------------------------------------------------------------------ C: Schweif
// Small sizes: one ring stroke weight per grid, two to three rings, no swelling.
const Cc = {
  16: { sw: 2.2, r1: 2, r2: 5.4, rings: [[0.5, 0.62], [0.8, 0.92]] },
  20: { sw: 2.6, r1: 2.5, r2: 6.8, rings: [[0.46, 0.57], [0.66, 0.77], [0.86, 0.96]] },
  24: { sw: 3, r1: 3, r2: 8.2, rings: [[0.42, 0.52], [0.6, 0.7], [0.78, 0.88]] },
  32: { sw: 3.8, r1: 4, r2: 11, rings: [[0.38, 0.46], [0.53, 0.61], [0.68, 0.76], [0.83, 0.91]] },
};
function schweif_small(n) {
  const k = Cc[n];
  if (!k) return null;
  const c = n / 2, { sw, r1, r2 } = k;
  const end = (84 * Math.PI) / 180, span = 2 * Math.PI - end;
  const pt = (t) => [c + r2 * Math.cos(-t * span), c + r2 * Math.sin(-t * span)];
  const arc = (a, b) => { const [x0, y0] = pt(a), [x1, y1] = pt(b); const large = (b - a) * span > Math.PI ? 1 : 0; return `M${x0.toFixed(2)} ${y0.toFixed(2)} A${r2} ${r2} 0 ${large} 0 ${x1.toFixed(2)} ${y1.toFixed(2)}`; };
  const sx = c + r1, hr = (r2 - r1) / 2;
  return svg(n, `<defs>${grad("p", "#2F2925", "#171412")}<linearGradient id="r" gradientUnits="userSpaceOnUse" x1="0" y1="${n * 0.15}" x2="0" y2="${n * 0.85}"><stop offset="0" stop-color="${P.rustHi}"/><stop offset="1" stop-color="${P.rustLo}"/></linearGradient></defs>
  <path d="${squircle(c, c, c - 0.5, 5, 360)}" fill="url(#p)" stroke="#6F6660" stroke-width="1"/>
  <g fill="none" stroke="url(#r)" stroke-width="${sw}">
    <circle cx="${c}" cy="${c}" r="${r1}"/>
    <path d="M${sx} ${c - r1} V${c + hr * 0.4} A${hr} ${hr} 0 0 0 ${c + r2} ${c + hr * 0.4} V${c}" stroke-linecap="round"/>
    <path d="${arc(0, 1)}" stroke-linecap="round"/>
  </g>
  ${k.rings.map(([a, b]) => `<path d="${arc(a, b)}" fill="none" stroke="#F7D9B6" stroke-width="${sw + 0.1}"/>`).join("")}`);
}

// ------------------------------------------------------------------ D: Siegel
// Small sizes: the seal grows, the flap is one line, the emblem becomes a
// cream envelope-muzzle with two ears.
const D = {
  16: { seal: [8, 9, 5.5], flap: 6, emb: 1 },
  20: { seal: [10, 11.25, 6.75], flap: 7.5, emb: 1.25 },
  24: { seal: [12, 13.25, 8], flap: 9, emb: 1.5 },
  32: { seal: [16, 17.5, 10.5], flap: 12, emb: 2 },
};
function siegel_small(n) {
  const k = D[n];
  if (!k) return null;
  const [cx, cy, R] = k.seal, e = k.emb, c = n / 2;
  const plate = squircle(c, c, c - 0.5, 5, 360);
  // emblem in units of e, centred at seal centre
    const envD = `M${cx - 3.5 * e} ${cy - 0.5 * e} L${cx} ${cy + 1.5 * e} L${cx + 3.5 * e} ${cy - 0.5 * e} V${cy + 3 * e} H${cx - 3.5 * e} Z`;
  return svg(n, `<defs>${grad("pp", "#FBF3E8", "#E6D2BA")}<radialGradient id="wx" cx="0.36" cy="0.3" r="0.85"><stop offset="0" stop-color="#EE7034"/><stop offset="1" stop-color="#A9340D"/></radialGradient></defs>
  <path d="${plate}" fill="url(#pp)" stroke="#9A7553" stroke-width="1"/>
  <path d="M1.5 ${k.flap * 0.55} L${c} ${k.flap + 1} L${n - 1.5} ${k.flap * 0.55}" fill="none" stroke="#C9A886" stroke-width="${n >= 24 ? 1 : 0.8}"/>
  <circle cx="${cx}" cy="${cy}" r="${R}" fill="url(#wx)"/>
  <path d="${envD}" fill="${P.cream}"/>`);
}

module.exports = { maske_small, versteck_small, schweif_small, siegel_small };
