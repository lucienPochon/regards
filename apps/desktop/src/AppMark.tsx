import { useEffect, useId, useRef, useState } from "react";

/**
 * The Regards mark, "Traced": an envelope drawn by one point, the agent,
 * which stops short of closing it (the last stroke is yours). The same
 * drawing as the app icon (src-tauri/icons/source); below 48 px, its small
 * cut, with a heavier line and a bigger point.
 *
 * - `draw`: it draws itself once, the flap first, then the point tracing
 *   the outline.
 * - `pulse`: the point then pulses, like something still at work.
 * - `label`: name it (it stands alone); without, it is decoration beside
 *   the word "Regards".
 *
 * With reduced motion it appears finished, and still.
 */
export function AppMark({
  size,
  draw = false,
  pulse = false,
  label,
}: {
  size: number;
  draw?: boolean;
  pulse?: boolean;
  label?: string;
}) {
  const id = `mark${useId().replace(/[^a-zA-Z0-9]/g, "")}`;
  const svg = useRef<SVGSVGElement>(null);
  const motion = useMotion();
  const drawing = draw && motion;
  const pulsing = pulse && motion;
  const cut = size < 48 ? SMALL : REGULAR;

  // The point's travel is SMIL, timed from the drawing's own start.
  useEffect(() => {
    if (drawing) svg.current?.setCurrentTime(0);
  }, [drawing]);

  const point = (
    <>
      {pulsing && <circle className="mark-ring" cx="0" cy="0" r={cut.point} fill="#5FD0AA" />}
      <circle cx="0" cy="0" r={cut.point} fill="#FFFFFF" />
    </>
  );
  return (
    <svg
      ref={svg}
      className={["mark", drawing ? "mark-draw" : "", pulsing ? "mark-pulse" : ""].join(" ")}
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      {...(label ? { role: "img", "aria-label": label } : { "aria-hidden": true })}
    >
      <defs>
        <linearGradient id={`${id}-tile`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#2E3236" />
          <stop offset="1" stopColor="#16181B" />
        </linearGradient>
        <linearGradient id={`${id}-sheen`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#FFFFFF" stopOpacity="0.1" />
          <stop offset="0.5" stopColor="#FFFFFF" stopOpacity="0" />
        </linearGradient>
        <clipPath id={`${id}-clip`}>
          <path d={TILE} />
        </clipPath>
      </defs>
      <path d={TILE} fill={`url(#${id}-tile)`} />
      <path d={TILE} fill={`url(#${id}-sheen)`} />
      <path d={TILE} fill="none" stroke="#FFFFFF" strokeOpacity="0.08" strokeWidth="8" clipPath={`url(#${id}-clip)`} />
      <path
        className="mark-flap"
        d={cut.flap}
        pathLength={100}
        fill="none"
        stroke="#5FD0AA"
        strokeWidth={cut.line}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path
        className="mark-outline"
        d={cut.outline}
        pathLength={100}
        fill="none"
        stroke="#5FD0AA"
        strokeWidth={cut.line}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {cut.halo > 0 && !pulsing && <circle cx={cut.end[0]} cy={cut.end[1]} r={cut.halo} fill="#5FD0AA" fillOpacity="0.16" />}
      {drawing ? (
        <g className="mark-point">
          {point}
          <animateMotion
            path={cut.outline}
            begin="0.6s"
            dur="1.5s"
            fill="freeze"
            calcMode="spline"
            keyPoints="0;1"
            keyTimes="0;1"
            keySplines="0.55 0.05 0.25 1"
          />
        </g>
      ) : (
        <g transform={`translate(${cut.end[0]} ${cut.end[1]})`}>{point}</g>
      )}
    </svg>
  );
}

/**
 * The window while the background service starts. It fades in only if that
 * takes a while (350 ms), so a quick start goes straight to the inbox.
 */
export function Launch() {
  return (
    <div className="launch">
      <AppMark size={96} draw pulse />
      <span className="launch-name" aria-hidden="true">
        Regards
      </span>
      <span className="launch-status" role="status">
        Starting Regards…
      </span>
    </div>
  );
}

/** Whether the system allows motion; follows the setting as it changes. */
function useMotion() {
  const query = "(prefers-reduced-motion: reduce)";
  const [motion, setMotion] = useState(() => !window.matchMedia?.(query).matches);
  useEffect(() => {
    const list = window.matchMedia?.(query);
    if (!list) return;
    const change = () => setMotion(!list.matches);
    list.addEventListener("change", change);
    return () => list.removeEventListener("change", change);
  }, []);
  return motion;
}

interface Cut {
  outline: string;
  flap: string;
  /** Where the point stops: the outline's end. */
  end: [number, number];
  line: number;
  point: number;
  /** The still halo around the point; none in the small cut. */
  halo: number;
}

/** On a 1024 grid: a 576 × 400 envelope, line 60, corners 76, point ⌀ 132. */
const REGULAR: Cut = {
  outline: "M224 596 V388 A76 76 0 0 1 300 312 H724 A76 76 0 0 1 800 388 V636 A76 76 0 0 1 724 712 H444",
  flap: "M262 350 L512 532 L762 350",
  end: [444, 712],
  line: 60,
  point: 66,
  halo: 112,
};

/** For 48 px and less: line 100, corners 90, point ⌀ 208, a wider gap. */
const SMALL: Cut = {
  outline: "M196 612 V406 A90 90 0 0 1 286 316 H738 A90 90 0 0 1 828 406 V632 A90 90 0 0 1 738 722 H468",
  flap: "M250 370 L512 548 L774 370",
  end: [468, 722],
  line: 100,
  point: 104,
  halo: 0,
};

/** The tile: a continuous-corner superellipse (n = 5), full bleed. */
const TILE = superellipse();

function superellipse() {
  const points: string[] = [];
  const steps = 192;
  for (let i = 0; i < steps; i += 1) {
    const t = (i / steps) * 2 * Math.PI;
    const c = Math.cos(t);
    const s = Math.sin(t);
    const x = 512 + 512 * Math.sign(c) * Math.abs(c) ** 0.4;
    const y = 512 + 512 * Math.sign(s) * Math.abs(s) ** 0.4;
    points.push(`${x.toFixed(1)} ${y.toFixed(1)}`);
  }
  return `M${points.join("L")}Z`;
}
