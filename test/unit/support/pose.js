// A phone held the way a player holds it, as the beta and gamma (degrees)
// deviceorientation would report. Built the physical way round with the
// spec's frames (device x to the right, y to the top, z out of the glass;
// world z up; R = Rz(α)·Rx(β)·Ry(γ) takes device to world): lying face up,
// turned so the picture is the right way up for `angle` (window.orientation:
// 90 is the phone turned anticlockwise), the wheel turned right by `turn`,
// then stood up to face the player, leaning `back` degrees from upright.
//
// The screen's roll then comes out as asin(sin(turn)·cos(back)): leaning
// back doesn't change which way it steers, only a little how far.

const DEG = Math.PI / 180;

export function pose({ angle = 90, turn = 0, back = 30 } = {}) {
  const a = (angle - turn) * DEG, t = (90 - back) * DEG;
  // R = Rx(t)·Rz(a); its bottom row is "up" in the phone's axes, which the
  // spec's R also gives as (−cos β sin γ, sin β, cos β cos γ).
  const ux = Math.sin(t) * Math.sin(a), uy = Math.sin(t) * Math.cos(a), uz = Math.cos(t);
  // gamma stays in [−90°, 90°): cos β takes the sign of uz.
  let beta = Math.asin(Math.max(-1, Math.min(1, uy))) / DEG;
  let gamma = Math.atan2(-ux, uz) / DEG;
  if (uz < 0) {
    beta = 180 - beta;
    if (beta >= 180) beta -= 360;
    gamma = Math.atan2(ux, -uz) / DEG;
  }
  return { alpha: 0, beta, gamma };
}

export const expectedRoll = ({ turn = 0, back = 30 } = {}) => Math.asin(Math.sin(turn * DEG) * Math.cos(back * DEG));
