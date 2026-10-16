// Master artwork for the four redpanda icon directions.
// Canvas: 1024 x 1024. Plate keyline: 64..960 (896), squircle n = 5.

const C = 512;

function squircle(cx, cy, half, n = 5, steps = 720) {
  const pts = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const c = Math.cos(t), s = Math.sin(t);
    const x = cx + half * Math.sign(c) * Math.pow(Math.abs(c), 2 / n);
    const y = cy + half * Math.sign(s) * Math.pow(Math.abs(s), 2 / n);
    pts.push(`${x.toFixed(2)},${y.toFixed(2)}`);
  }
  return `M${pts.join("L")}Z`;
}

const wrap = (body, size = 1024) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 1024 1024">${body}</svg>`;

const mirror = (d) => `<g transform="translate(1024 0) scale(-1 1)">${d}</g>`;

// Palette, derived from the app tokens (--accent #b45309, --text #1c1917, warm neutrals)
const P = {
  rustHi: "#F08040", rust: "#D9581E", rustLo: "#B8400F",
  fur: "#8E2F0E", furLo: "#6E220A",
  cream: "#FFF8EF", creamLo: "#F4E3CF",
  ink: "#1C1917",
};

// ---------------------------------------------------------------- the face mark
// Envelope = face. Body = white muzzle and cheeks. Flap = rust forehead that
// runs down between the eyes to the nose (the red panda's widow's peak).
// Drawn on a 1024 canvas, centred, about 600 wide. `id` prefixes gradients.
function face(id, { brows = true, tears = true } = {}) {
  const ear = `
    <path d="M232 470 C214 368 226 262 276 236 C318 216 392 266 452 352 Z" fill="url(#${id}cr)"/>
    <path d="M276 420 C268 352 276 300 300 290 C326 280 368 314 396 352 Z" fill="${P.fur}"/>`;
  return `
  <defs>
    <linearGradient id="${id}cr" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFBF6"/><stop offset="1" stop-color="${P.creamLo}"/>
    </linearGradient>
    <linearGradient id="${id}fl" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="${P.furLo}"/><stop offset="1" stop-color="${P.fur}"/>
    </linearGradient>
  </defs>
  ${ear}${mirror(ear)}
  <rect x="216" y="356" width="592" height="424" rx="92" fill="url(#${id}cr)"/>
  <path d="M290 356 H734 Q800 356 770 404 L566 610 Q512 660 458 610 L254 404 Q224 356 290 356 Z" fill="url(#${id}fl)"/>
  ${brows ? `<ellipse cx="372" cy="430" rx="42" ry="26" transform="rotate(20 372 430)" fill="${P.cream}"/>
  <ellipse cx="652" cy="430" rx="42" ry="26" transform="rotate(-20 652 430)" fill="${P.cream}"/>` : ""}
  ${tears ? `<path d="M366 612 Q346 668 312 712" fill="none" stroke="${P.rustLo}" stroke-width="40" stroke-linecap="round"/>
  <path d="M658 612 Q678 668 712 712" fill="none" stroke="${P.rustLo}" stroke-width="40" stroke-linecap="round"/>` : ""}
  <circle cx="372" cy="580" r="40" fill="${P.ink}"/>
  <circle cx="652" cy="580" r="40" fill="${P.ink}"/>
  <path d="M476 664 H548 Q572 664 558 684 L530 716 Q512 734 494 716 L466 684 Q452 664 476 664 Z" fill="${P.ink}"/>`;
}

// ---------------------------------------------------------------- A: Maske
// Only the white markings are drawn, the plate is the fur (the WWF method).
// The white muzzle and cheeks form an open envelope; the rust forehead comes
// down into the flap and ends in the nose.
function mask(id) {
  const ear = `
    <path d="M206 420 C184 300 206 196 262 176 C312 158 392 214 446 300 Z" fill="${P.cream}"/>
    <path d="M250 378 C238 300 252 246 278 238 C306 230 350 268 382 306 Z" fill="${P.fur}"/>`;
  return `
  <defs>
    <linearGradient id="${id}cr" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFBF6"/><stop offset="1" stop-color="${P.creamLo}"/>
    </linearGradient>
  </defs>
  ${ear}${mirror(ear)}
  <ellipse cx="380" cy="404" rx="50" ry="30" transform="rotate(14 380 404)" fill="${P.cream}"/>
  <ellipse cx="644" cy="404" rx="50" ry="30" transform="rotate(-14 644 404)" fill="${P.cream}"/>
  <path d="M200 548 Q200 470 270 500 L470 642 Q512 670 554 642 L754 500 Q824 470 824 548 V752 Q824 840 736 840 H288 Q200 840 200 752 Z" fill="url(#${id}cr)"/>
  <circle cx="380" cy="508" r="42" fill="${P.ink}"/>
  <circle cx="644" cy="508" r="42" fill="${P.ink}"/>
  <path d="M472 600 H552 Q578 600 562 622 L530 660 Q512 680 494 660 L462 622 Q446 600 472 600 Z" fill="${P.ink}"/>`;
}

function maske() {
  return wrap(`
  <defs>
    <linearGradient id="hl" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="0.35"/><stop offset="0.5" stop-color="#fff" stop-opacity="0"/></linearGradient>
    <linearGradient id="pl" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="${P.rustHi}"/><stop offset="1" stop-color="${P.rustLo}"/>
    </linearGradient>
  </defs>
  <path d="${squircle(C, C, 448)}" fill="url(#pl)"/>
  <path d="${squircle(C, C, 446)}" fill="none" stroke="url(#hl)" stroke-width="5"/>
  <g transform="translate(512 520) scale(0.86) translate(-512 -508)">${mask("a")}</g>
  `);
}

// ---------------------------------------------------------------- B: Versteck
// Windows 11 native: no plate, layered material, light from the top.
// The panda hides in the envelope; its white muzzle continues into the pocket.
function versteck() {
  const ear = `
    <path d="M300 420 C270 300 282 190 332 168 C380 148 452 214 500 300 Z" fill="${P.cream}"/>
    <path d="M326 360 C316 290 326 236 350 228 C376 220 412 256 440 300 Z" fill="${P.furLo}"/>`;
  return wrap(`
  <defs>
    <linearGradient id="bk" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#E2C3A4"/><stop offset="1" stop-color="#CFA885"/>
    </linearGradient>
    <linearGradient id="hd" x1="0.3" y1="0" x2="0.7" y2="1">
      <stop offset="0" stop-color="${P.rustHi}"/><stop offset="1" stop-color="${P.rustLo}"/>
    </linearGradient>
    <linearGradient id="fr" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF"/><stop offset="1" stop-color="#F5E6D6"/>
    </linearGradient>
    <filter id="sh" x="-10%" y="-30%" width="120%" height="160%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="12"/>
      <feOffset dy="-8"/>
      <feComponentTransfer><feFuncA type="linear" slope="0.25"/></feComponentTransfer>
      <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="drop" x="-20%" y="-20%" width="140%" height="150%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="16"/>
      <feOffset dy="12"/>
      <feComponentTransfer><feFuncA type="linear" slope="0.24"/></feComponentTransfer>
      <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <g transform="translate(0 -22)"><g filter="url(#drop)">
    <rect x="96" y="440" width="832" height="464" rx="80" fill="url(#bk)" stroke="#9C7350" stroke-opacity="0.45" stroke-width="6"/>
    ${ear}${mirror(ear)}
    <path d="M512 262 C680 262 780 360 780 520 C780 610 760 700 720 760 H304 C264 700 244 610 244 520 C244 360 344 262 512 262 Z" fill="url(#hd)"/>
    <ellipse cx="404" cy="420" rx="44" ry="28" transform="rotate(16 404 420)" fill="${P.cream}"/>
    <ellipse cx="620" cy="420" rx="44" ry="28" transform="rotate(-16 620 420)" fill="${P.cream}"/>
    <path d="M512 520 C590 520 640 560 660 640 L560 720 H464 L364 640 C384 560 434 520 512 520 Z" fill="${P.cream}"/>
    <circle cx="404" cy="500" r="36" fill="${P.ink}"/>
    <circle cx="620" cy="500" r="36" fill="${P.ink}"/>
    <path d="M474 584 H550 Q574 584 560 604 L530 638 Q512 656 494 638 L464 604 Q450 584 474 584 Z" fill="${P.ink}"/>
    <g filter="url(#sh)">
      <path d="M96 572 Q96 518 144 542 L462 700 Q512 724 562 700 L880 542 Q928 518 928 572 V824 Q928 904 848 904 H176 Q96 904 96 824 Z" fill="url(#fr)" stroke="#9C7350" stroke-opacity="0.4" stroke-width="6" stroke-linejoin="round"/>
    </g>
  </g></g>
  `);
}

// ---------------------------------------------------------------- C: Schweif
// The @ of every address, drawn as a ringed red panda tail. The tail swells
// from the stem towards a bushy tip; the rings follow its width exactly.
function tail({ r = 292, w0 = 58, w1 = 70, end = 84, rings = [[0.36, 0.43], [0.50, 0.57], [0.64, 0.71], [0.78, 0.85], [0.92, 0.985]] } = {}) {
  const span = 2 * Math.PI - (end * Math.PI) / 180;
  const th = (t) => -t * span;
  const ease = (t) => t * t * (3 - 2 * t);
  const w = (t) => w0 + (w1 - w0) * ease(t);
  const pt = (t, rr) => [C + rr * Math.cos(th(t)), C + rr * Math.sin(th(t))];
  const band = (a, b, steps = 90) => {
    const o = [], i = [];
    for (let k = 0; k <= steps; k++) {
      const t = a + ((b - a) * k) / steps;
      o.push(pt(t, r + w(t))); i.push(pt(t, r - w(t)));
    }
    return "M" + [...o, ...i.reverse()].map((p) => p.map((v) => v.toFixed(1)).join(",")).join("L") + "Z";
  };
  const [tx, ty] = pt(1, r);
  return { body: band(0, 1, 360), rings: rings.map(([a, b]) => band(a, b)), tip: [tx, ty, w(1)], w0 };
}

function schweif() {
  const T = tail();
  return wrap(`
  <defs>
    <linearGradient id="hl" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="0.16"/><stop offset="0.5" stop-color="#fff" stop-opacity="0"/></linearGradient>
    <linearGradient id="pl" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#2F2925"/><stop offset="1" stop-color="#171412"/>
    </linearGradient>
    <linearGradient id="ru" gradientUnits="userSpaceOnUse" x1="0" y1="180" x2="0" y2="880">
      <stop offset="0" stop-color="${P.rustHi}"/><stop offset="1" stop-color="${P.rustLo}"/>
    </linearGradient>
  </defs>
  <path d="${squircle(C, C, 448)}" fill="url(#pl)"/>
  <path d="${squircle(C, C, 446)}" fill="none" stroke="#fff" stroke-opacity="0.13" stroke-width="5"/>
  <path d="${squircle(C, C, 446)}" fill="none" stroke="url(#hl)" stroke-width="5"/>
  <circle cx="512" cy="512" r="118" fill="none" stroke="url(#ru)" stroke-width="116"/>
  <path d="M630 394 V560 Q630 640 717 640 Q804 640 804 560 V512" fill="none" stroke="url(#ru)" stroke-width="116" stroke-linecap="round"/>
  <path d="${T.body}" fill="url(#ru)"/>
  <circle cx="${T.tip[0].toFixed(1)}" cy="${T.tip[1].toFixed(1)}" r="${T.tip[2].toFixed(1)}" fill="url(#ru)"/>
  ${T.rings.map((d) => `<path d="${d}" fill="#F7D9B6"/>`).join("")}
  `);
}

// ---------------------------------------------------------------- D: Siegel
// The plate is the letter. The seal says: closed, private, yours.
function siegel() {
  const plate = squircle(C, C, 448);
  return wrap(`
  <defs>
    <linearGradient id="pp" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FBF3E8"/><stop offset="1" stop-color="#EAD8C2"/>
    </linearGradient>
    <linearGradient id="fl" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFDF9"/><stop offset="1" stop-color="#F6EADB"/>
    </linearGradient>
    <radialGradient id="wx" cx="0.36" cy="0.3" r="0.85">
      <stop offset="0" stop-color="#EE7034"/><stop offset="1" stop-color="#A9340D"/>
    </radialGradient>
    <clipPath id="cp"><path d="${plate}"/></clipPath>
    <filter id="sd" x="-30%" y="-30%" width="160%" height="160%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="18"/><feOffset dy="14"/>
      <feComponentTransfer><feFuncA type="linear" slope="0.32"/></feComponentTransfer>
      <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
    <filter id="fs" x="-10%" y="-10%" width="120%" height="140%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="10"/><feOffset dy="8"/>
      <feComponentTransfer><feFuncA type="linear" slope="0.12"/></feComponentTransfer>
      <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <path d="${plate}" fill="url(#pp)"/>
  <g clip-path="url(#cp)">
    <path d="M40 260 L512 560 L984 260 V40 H40 Z" fill="url(#fl)" filter="url(#fs)"/>
  </g>
  <path d="${squircle(C, C, 447)}" fill="none" stroke="#5A3A1E" stroke-opacity="0.3" stroke-width="4"/>
  <g filter="url(#sd)">
    <circle cx="512" cy="566" r="252" fill="url(#wx)"/>
  </g>
  <circle cx="512" cy="566" r="214" fill="none" stroke="#7E260A" stroke-opacity="0.35" stroke-width="10"/>
  <g transform="translate(512 586) scale(0.52) translate(-512 -508)"><path d="M200 548 Q200 470 270 500 L470 642 Q512 670 554 642 L754 500 Q824 470 824 548 V752 Q824 840 736 840 H288 Q200 840 200 752 Z" fill="none"/>${mask("d")}</g>
  `);
}

module.exports = { tail, mask, maske, versteck, schweif, siegel, squircle, face, P, wrap, mirror };
