//! SVG 形状解析与 Direct2D PathGeometry 转换
//!
//! 支持的形状（来自 icons_svg_defs::SvgShape）：
//! - Path：M/L/H/V/C/S/Q/T/A/Z 命令（path d="..."）
//! - Circle：转 4 段三次贝塞尔
//! - Rect：转 M/L/Z 路径（带 rx 圆角时插值贝塞尔）
//! - Line：转 M/L 路径
//!
//! 不支持：
//! - SVG gradient、动画、滤镜、clipPath
//! - 嵌套 `<defs>`/`<use>`
//!
//! 来源：Lucide (ISC) + Devicon (MIT) — 商业友好。

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::ID2D1Factory;

pub(crate) use super::icons_svg_defs::{SvgDef, SvgShape};

/// 解析 "#RRGGBB" 或 "#RGB" 为 (r,g,b,a) (范围 0.0~1.0)
pub(crate) fn parse_hex_color(hex: &str) -> (f32, f32, f32, f32) {
    let s = hex.trim_start_matches('#');
    let parse_pair = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    if s.len() == 6 {
        (parse_pair(0), parse_pair(2), parse_pair(4), 1.0)
    } else if s.len() == 3 {
        let r = u8::from_str_radix(&s[0..1], 16).unwrap_or(0) * 17;
        let g = u8::from_str_radix(&s[1..2], 16).unwrap_or(0) * 17;
        let b = u8::from_str_radix(&s[2..3], 16).unwrap_or(0) * 17;
        (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    } else {
        (1.0, 1.0, 1.0, 1.0)
    }
}

// ===========================================================================
// SVG path 解析（M/L/H/V/C/S/Q/T/A/Z）
// ===========================================================================

struct PathParser {
    chars: Vec<char>,
    pos: usize,
}

impl PathParser {
    fn new(d: &str) -> Self {
        Self {
            chars: d.chars().collect(),
            pos: 0,
        }
    }

    fn skip_whitespace_and_commas(&mut self) {
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_whitespace() || c == ',' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn read_number(&mut self) -> Option<f32> {
        self.skip_whitespace_and_commas();
        let start = self.pos;
        if matches!(self.peek(), Some('+') | Some('-')) {
            self.pos += 1;
        }
        while self.pos < self.chars.len() {
            let c = self.chars[self.pos];
            if c.is_ascii_digit() || c == '.' {
                self.pos += 1;
            } else if c == 'e' || c == 'E' {
                self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
        if start == self.pos {
            return None;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        s.parse::<f32>().ok()
    }
}

/// 把 path data 字符串通过回调函数转为路径构建命令
/// 内部自动将 Q/T 命令转换为三次贝塞尔（C），回调只收到 M/L/C/Z
pub(crate) fn parse_svg_path<F>(d: &str, mut cb: F) -> Result<(), String>
where
    F: FnMut(char, &[f32]) -> Result<(), String>,
{
    let mut p = PathParser::new(d);
    let mut last_cmd = ' ';
    let mut cx = 0.0f32;
    let mut cy = 0.0f32;
    let mut start_x = 0.0f32;
    let mut start_y = 0.0f32;
    let mut last_cubic_cx = 0.0f32;
    let mut last_cubic_cy = 0.0f32;
    let mut last_quad_cx = 0.0f32;
    let mut last_quad_cy = 0.0f32;
    let mut have_cubic = false;
    let mut have_quad = false;

    while p.pos < p.chars.len() {
        p.skip_whitespace_and_commas();
        if p.pos >= p.chars.len() {
            break;
        }
        let c = p.peek().unwrap();
        let cmd = if c.is_alphabetic() {
            p.next().unwrap()
        } else {
            last_cmd
        };
        let upper = cmd.to_ascii_uppercase();
        let rel = cmd.is_ascii_lowercase();
        last_cmd = cmd;

        match upper {
            'M' => {
                let x = p.read_number().ok_or("M 缺 x")?;
                let y = p.read_number().ok_or("M 缺 y")?;
                let (mx, my) = if rel { (cx + x, cy + y) } else { (x, y) };
                cb('M', &[mx, my])?;
                cx = mx;
                cy = my;
                start_x = mx;
                start_y = my;
                have_cubic = false;
                have_quad = false;
                last_cmd = if rel { 'l' } else { 'L' };
            }
            'L' => {
                let x = p.read_number().ok_or("L 缺 x")?;
                let y = p.read_number().ok_or("L 缺 y")?;
                let (mx, my) = if rel { (cx + x, cy + y) } else { (x, y) };
                cb('L', &[mx, my])?;
                cx = mx;
                cy = my;
                have_cubic = false;
                have_quad = false;
            }
            'H' => {
                let x = p.read_number().ok_or("H 缺 x")?;
                let mx = if rel { cx + x } else { x };
                cb('L', &[mx, cy])?;
                cx = mx;
                have_cubic = false;
                have_quad = false;
            }
            'V' => {
                let y = p.read_number().ok_or("V 缺 y")?;
                let my = if rel { cy + y } else { y };
                cb('L', &[cx, my])?;
                cy = my;
                have_cubic = false;
                have_quad = false;
            }
            'C' => {
                let c1x = p.read_number().ok_or("C 缺 c1x")?;
                let c1y = p.read_number().ok_or("C 缺 c1y")?;
                let c2x = p.read_number().ok_or("C 缺 c2x")?;
                let c2y = p.read_number().ok_or("C 缺 c2y")?;
                let x = p.read_number().ok_or("C 缺 x")?;
                let y = p.read_number().ok_or("C 缺 y")?;
                let p1 = if rel {
                    (cx + c1x, cy + c1y)
                } else {
                    (c1x, c1y)
                };
                let p2 = if rel {
                    (cx + c2x, cy + c2y)
                } else {
                    (c2x, c2y)
                };
                let pp = if rel { (cx + x, cy + y) } else { (x, y) };
                cb('C', &[p1.0, p1.1, p2.0, p2.1, pp.0, pp.1])?;
                last_cubic_cx = p2.0;
                last_cubic_cy = p2.1;
                cx = pp.0;
                cy = pp.1;
                have_cubic = true;
                have_quad = false;
            }
            'S' => {
                let c2x = p.read_number().ok_or("S 缺 c2x")?;
                let c2y = p.read_number().ok_or("S 缺 c2y")?;
                let x = p.read_number().ok_or("S 缺 x")?;
                let y = p.read_number().ok_or("S 缺 y")?;
                let (c1x, c1y) = if have_cubic {
                    (2.0 * cx - last_cubic_cx, 2.0 * cy - last_cubic_cy)
                } else {
                    (cx, cy)
                };
                let p2 = if rel {
                    (cx + c2x, cy + c2y)
                } else {
                    (c2x, c2y)
                };
                let pp = if rel { (cx + x, cy + y) } else { (x, y) };
                cb('C', &[c1x, c1y, p2.0, p2.1, pp.0, pp.1])?;
                last_cubic_cx = p2.0;
                last_cubic_cy = p2.1;
                cx = pp.0;
                cy = pp.1;
                have_cubic = true;
                have_quad = false;
            }
            'Q' => {
                // 二次贝塞尔转三次贝塞尔（数学等价）
                // 起点 P0 = (cx, cy)，控制点 P1 = (args[0], args[1])，终点 P2 = (args[2], args[3])
                // 等价三次贝塞尔控制点：
                //   CP1 = P0 + 2/3 * (P1 - P0)
                //   CP2 = P2 + 2/3 * (P1 - P2)
                let cpx = p.read_number().ok_or("Q 缺 cpx")?;
                let cpy = p.read_number().ok_or("Q 缺 cpy")?;
                let x = p.read_number().ok_or("Q 缺 x")?;
                let y = p.read_number().ok_or("Q 缺 y")?;
                let cp = if rel {
                    (cx + cpx, cy + cpy)
                } else {
                    (cpx, cpy)
                };
                let pp = if rel { (cx + x, cy + y) } else { (x, y) };
                let cp1x = cx + 2.0 / 3.0 * (cp.0 - cx);
                let cp1y = cy + 2.0 / 3.0 * (cp.1 - cy);
                let cp2x = pp.0 + 2.0 / 3.0 * (cp.0 - pp.0);
                let cp2y = pp.1 + 2.0 / 3.0 * (cp.1 - pp.1);
                cb('C', &[cp1x, cp1y, cp2x, cp2y, pp.0, pp.1])?;
                last_cubic_cx = cp2x;
                last_cubic_cy = cp2y;
                last_quad_cx = cp.0;
                last_quad_cy = cp.1;
                cx = pp.0;
                cy = pp.1;
                have_quad = true;
                have_cubic = true;
            }
            'T' => {
                // T 后的控制点 P1 = 2 * 当前位置 - 上一个 Q 的控制点
                let x = p.read_number().ok_or("T 缺 x")?;
                let y = p.read_number().ok_or("T 缺 y")?;
                let cp = if have_quad {
                    (2.0 * cx - last_quad_cx, 2.0 * cy - last_quad_cy)
                } else {
                    (cx, cy)
                };
                let pp = if rel { (cx + x, cy + y) } else { (x, y) };
                let cp1x = cx + 2.0 / 3.0 * (cp.0 - cx);
                let cp1y = cy + 2.0 / 3.0 * (cp.1 - cy);
                let cp2x = pp.0 + 2.0 / 3.0 * (cp.0 - pp.0);
                let cp2y = pp.1 + 2.0 / 3.0 * (cp.1 - pp.1);
                cb('C', &[cp1x, cp1y, cp2x, cp2y, pp.0, pp.1])?;
                last_cubic_cx = cp2x;
                last_cubic_cy = cp2y;
                last_quad_cx = cp.0;
                last_quad_cy = cp.1;
                cx = pp.0;
                cy = pp.1;
                have_quad = true;
                have_cubic = true;
            }
            'A' => {
                let mut rx = p.read_number().ok_or("A 缺 rx")?.abs();
                let mut ry = p.read_number().ok_or("A 缺 ry")?.abs();
                let rot = p.read_number().ok_or("A 缺 rot")?;
                let large = p.read_number().ok_or("A 缺 large")? != 0.0;
                let sweep = p.read_number().ok_or("A 缺 sweep")? != 0.0;
                let x = p.read_number().ok_or("A 缺 x")?;
                let y = p.read_number().ok_or("A 缺 y")?;
                let pp = if rel { (cx + x, cy + y) } else { (x, y) };
                let degenerate = rx < 1e-6
                    || ry < 1e-6
                    || ((pp.0 - cx).abs() < 1e-6 && (pp.1 - cy).abs() < 1e-6);
                if degenerate {
                    // 退化情况按规范处理为直线
                    cb('L', &[pp.0, pp.1])?;
                } else {
                    // 端点参数化 → 中心参数化（SVG 规范 F.6.5），再转为三次贝塞尔
                    fn vec_angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
                        let dot = ux * vx + uy * vy;
                        let len = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
                        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
                        if ux * vy - uy * vx < 0.0 {
                            a = -a;
                        }
                        a
                    }
                    let phi = rot.to_radians();
                    let (sin_phi, cos_phi) = phi.sin_cos();
                    let dx = (cx - pp.0) / 2.0;
                    let dy = (cy - pp.1) / 2.0;
                    let x1p = cos_phi * dx + sin_phi * dy;
                    let y1p = -sin_phi * dx + cos_phi * dy;
                    // 半径不足时按规范放大
                    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
                    if lambda > 1.0 {
                        let s = lambda.sqrt();
                        rx *= s;
                        ry *= s;
                    }
                    let rx2 = rx * rx;
                    let ry2 = ry * ry;
                    let x1p2 = x1p * x1p;
                    let y1p2 = y1p * y1p;
                    let num = (rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2).max(0.0);
                    let den = rx2 * y1p2 + ry2 * x1p2;
                    let sign = if large != sweep { 1.0 } else { -1.0 };
                    let coef = sign * (num / den).sqrt();
                    let cxp = coef * rx * y1p / ry;
                    let cyp = -coef * ry * x1p / rx;
                    let ccx = cos_phi * cxp - sin_phi * cyp + (cx + pp.0) / 2.0;
                    let ccy = sin_phi * cxp + cos_phi * cyp + (cy + pp.1) / 2.0;
                    let ux = (x1p - cxp) / rx;
                    let uy = (y1p - cyp) / ry;
                    let theta1 = vec_angle(1.0, 0.0, ux, uy);
                    let mut dtheta = vec_angle(ux, uy, -ux, -uy);
                    let tau = std::f32::consts::TAU;
                    if !sweep && dtheta > 0.0 {
                        dtheta -= tau;
                    } else if sweep && dtheta < 0.0 {
                        dtheta += tau;
                    }
                    // 按 ≤90° 分段，每段用一条三次贝塞尔逼近
                    let segs = ((dtheta.abs() / std::f32::consts::FRAC_PI_2).ceil() as i32).max(1);
                    let delta = dtheta / segs as f32;
                    let alpha = 4.0 / 3.0 * (delta / 4.0).tan();
                    let mut theta = theta1;
                    for _ in 0..segs {
                        let (s1, c1) = theta.sin_cos();
                        let (s2, c2) = (theta + delta).sin_cos();
                        let p1x = ccx + rx * c1 * cos_phi - ry * s1 * sin_phi;
                        let p1y = ccy + rx * c1 * sin_phi + ry * s1 * cos_phi;
                        let p2x = ccx + rx * c2 * cos_phi - ry * s2 * sin_phi;
                        let p2y = ccy + rx * c2 * sin_phi + ry * s2 * cos_phi;
                        let d1x = -rx * s1 * cos_phi - ry * c1 * sin_phi;
                        let d1y = -rx * s1 * sin_phi + ry * c1 * cos_phi;
                        let d2x = -rx * s2 * cos_phi - ry * c2 * sin_phi;
                        let d2y = -rx * s2 * sin_phi + ry * c2 * cos_phi;
                        cb(
                            'C',
                            &[
                                p1x + alpha * d1x,
                                p1y + alpha * d1y,
                                p2x - alpha * d2x,
                                p2y - alpha * d2y,
                                p2x,
                                p2y,
                            ],
                        )?;
                        theta += delta;
                    }
                }
                cx = pp.0;
                cy = pp.1;
                have_cubic = false;
                have_quad = false;
            }
            'Z' => {
                cb('Z', &[])?;
                cx = start_x;
                cy = start_y;
                have_cubic = false;
                have_quad = false;
            }
            _ => return Err(format!("未知命令 {}", cmd)),
        }
    }
    Ok(())
}

// ===========================================================================
// 形状到 PathGeometry 转换
// ===========================================================================

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

/// 把一个 SvgShape 构建为一个 PathGeometry
/// fill = Some 视为实心（用于 Devicon 风格多色填充）
/// fill = None 视为描边轮廓（用于 Lucide 风格）
pub(crate) fn build_shape_geometry(
    factory: &ID2D1Factory,
    shape: &SvgShape,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    use windows::Win32::Graphics::Direct2D::Common::D2D1_BEZIER_SEGMENT;
    use windows::Win32::Graphics::Direct2D::ID2D1PathGeometry;

    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()? };

    let (d, fill) = match shape {
        SvgShape::Path(d, fill) => (*d, fill.map(|s| s.to_string())),
        SvgShape::Line(_, _, _, _) => {
            // 单独 Line 转 M/L 路径
            return build_line_geometry(factory, shape);
        }
        SvgShape::Rect(_, _, _, _, _, _) => {
            return build_rect_geometry(factory, shape);
        }
        SvgShape::Circle(_, _, _, _) => {
            return build_circle_geometry(factory, shape);
        }
    };

    let begin = if fill.is_some() {
        D2D1_FIGURE_BEGIN_FILLED
    } else {
        D2D1_FIGURE_BEGIN_HOLLOW
    };

    unsafe {
        let sink = geo.Open()?;
        let mut has_open_figure = false;
        let result = parse_svg_path(d, |cmd, args| {
            match cmd {
                'M' => {
                    if has_open_figure {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(pt(args[0], args[1]), begin);
                    has_open_figure = true;
                }
                'L' => {
                    sink.AddLine(pt(args[0], args[1]));
                }
                'C' => {
                    let seg = D2D1_BEZIER_SEGMENT {
                        point1: pt(args[0], args[1]),
                        point2: pt(args[2], args[3]),
                        point3: pt(args[4], args[5]),
                    };
                    sink.AddBezier(&seg);
                }
                'Z' if has_open_figure => {
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    has_open_figure = false;
                }
                _ => {}
            }
            Ok(())
        });
        if let Err(e) = result {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005u32 as i32),
                e,
            ));
        }
        if has_open_figure {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close()?;
    }
    Ok(geo)
}

fn build_line_geometry(
    factory: &ID2D1Factory,
    shape: &SvgShape,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    use windows::Win32::Graphics::Direct2D::ID2D1PathGeometry;
    let (x1, y1, x2, y2) = match shape {
        SvgShape::Line(x1, y1, x2, y2) => (*x1, *y1, *x2, *y2),
        _ => unreachable!(),
    };
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()? };
    unsafe {
        let sink = geo.Open()?;
        sink.BeginFigure(pt(x1, y1), D2D1_FIGURE_BEGIN_HOLLOW);
        sink.AddLine(pt(x2, y2));
        sink.EndFigure(D2D1_FIGURE_END_OPEN);
        sink.Close()?;
    }
    Ok(geo)
}

fn build_rect_geometry(
    factory: &ID2D1Factory,
    shape: &SvgShape,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    use windows::Win32::Graphics::Direct2D::Common::D2D1_BEZIER_SEGMENT;
    use windows::Win32::Graphics::Direct2D::ID2D1PathGeometry;
    let (x, y, w, h, _fill, rx_opt) = match shape {
        SvgShape::Rect(x, y, w, h, fill, rx) => (*x, *y, *w, *h, fill, *rx),
        _ => unreachable!(),
    };
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()? };
    let begin = if _fill.is_some() {
        D2D1_FIGURE_BEGIN_FILLED
    } else {
        D2D1_FIGURE_BEGIN_HOLLOW
    };
    let rx = rx_opt.unwrap_or(0.0).min(w / 2.0).min(h / 2.0);
    unsafe {
        let sink = geo.Open()?;
        if rx > 0.0 {
            // 圆角矩形（4 个角 + 直线段）
            // 圆角近似系数 ~ 0.5523 (cubic bezier 圆)
            let k = rx * 0.5523_f32;
            sink.BeginFigure(pt(x + rx, y), begin);
            // 顶边
            sink.AddLine(pt(x + w - rx, y));
            // 右上圆角
            let seg = D2D1_BEZIER_SEGMENT {
                point1: pt(x + w - rx + k, y),
                point2: pt(x + w, y + rx - k),
                point3: pt(x + w, y + rx),
            };
            sink.AddBezier(&seg);
            // 右边
            sink.AddLine(pt(x + w, y + h - rx));
            // 右下圆角
            let seg = D2D1_BEZIER_SEGMENT {
                point1: pt(x + w, y + h - rx + k),
                point2: pt(x + w - rx + k, y + h),
                point3: pt(x + w - rx, y + h),
            };
            sink.AddBezier(&seg);
            // 底边
            sink.AddLine(pt(x + rx, y + h));
            // 左下圆角
            let seg = D2D1_BEZIER_SEGMENT {
                point1: pt(x + rx - k, y + h),
                point2: pt(x, y + h - rx + k),
                point3: pt(x, y + h - rx),
            };
            sink.AddBezier(&seg);
            // 左边
            sink.AddLine(pt(x, y + rx));
            // 左上圆角
            let seg = D2D1_BEZIER_SEGMENT {
                point1: pt(x, y + rx - k),
                point2: pt(x + rx - k, y),
                point3: pt(x + rx, y),
            };
            sink.AddBezier(&seg);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        } else {
            sink.BeginFigure(pt(x, y), begin);
            sink.AddLine(pt(x + w, y));
            sink.AddLine(pt(x + w, y + h));
            sink.AddLine(pt(x, y + h));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        }
        sink.Close()?;
    }
    Ok(geo)
}

fn build_circle_geometry(
    factory: &ID2D1Factory,
    shape: &SvgShape,
) -> windows::core::Result<windows::Win32::Graphics::Direct2D::ID2D1PathGeometry> {
    use windows::Win32::Graphics::Direct2D::Common::D2D1_BEZIER_SEGMENT;
    use windows::Win32::Graphics::Direct2D::ID2D1PathGeometry;
    let (cx, cy, r, fill) = match shape {
        SvgShape::Circle(cx, cy, r, fill) => (*cx, *cy, *r, fill),
        _ => unreachable!(),
    };
    let geo: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()? };
    let begin = if fill.is_some() {
        D2D1_FIGURE_BEGIN_FILLED
    } else {
        D2D1_FIGURE_BEGIN_HOLLOW
    };
    // 4 段三次贝塞尔近似圆，k = r * 0.5523
    let k = r * 0.5523_f32;
    unsafe {
        let sink = geo.Open()?;
        sink.BeginFigure(pt(cx - r, cy), begin);
        // 左 -> 上
        let seg = D2D1_BEZIER_SEGMENT {
            point1: pt(cx - r, cy - k),
            point2: pt(cx - k, cy - r),
            point3: pt(cx, cy - r),
        };
        sink.AddBezier(&seg);
        // 上 -> 右
        let seg = D2D1_BEZIER_SEGMENT {
            point1: pt(cx + k, cy - r),
            point2: pt(cx + r, cy - k),
            point3: pt(cx + r, cy),
        };
        sink.AddBezier(&seg);
        // 右 -> 下
        let seg = D2D1_BEZIER_SEGMENT {
            point1: pt(cx + r, cy + k),
            point2: pt(cx + k, cy + r),
            point3: pt(cx, cy + r),
        };
        sink.AddBezier(&seg);
        // 下 -> 左
        let seg = D2D1_BEZIER_SEGMENT {
            point1: pt(cx - k, cy + r),
            point2: pt(cx - r, cy + k),
            point3: pt(cx - r, cy),
        };
        sink.AddBezier(&seg);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
    }
    Ok(geo)
}

/// 把 SvgDef 解析为多个 (geometry, fill_color) 列表
/// 同一 shape 的多个 figure 共享一个 fill 颜色
#[allow(clippy::type_complexity)]
pub(crate) fn build_def(
    factory: &ID2D1Factory,
    def: &SvgDef,
) -> Vec<(
    windows::Win32::Graphics::Direct2D::ID2D1PathGeometry,
    Option<(f32, f32, f32, f32)>,
)> {
    let mut out = Vec::new();
    for shape in def.shapes {
        let fill = match shape {
            SvgShape::Path(_, f) => f.map(parse_hex_color),
            SvgShape::Circle(_, _, _, f) => f.map(parse_hex_color),
            SvgShape::Rect(_, _, _, _, f, _) => f.map(parse_hex_color),
            SvgShape::Line(_, _, _, _) => None,
        };
        if let Ok(geo) = build_shape_geometry(factory, shape) {
            out.push((geo, fill));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_color_6() {
        let (r, g, b, a) = parse_hex_color("#3776AB");
        assert!((r - 0x37 as f32 / 255.0).abs() < 0.01);
        assert!((g - 0x76 as f32 / 255.0).abs() < 0.01);
        assert!((b - 0xAB as f32 / 255.0).abs() < 0.01);
        assert_eq!(a, 1.0);
    }

    #[test]
    fn hex_color_3() {
        let (r, g, b, _) = parse_hex_color("#fff");
        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 1.0).abs() < 0.01);
        assert!((b - 1.0).abs() < 0.01);
    }

    #[test]
    fn hex_color_invalid_returns_black() {
        // 长度是 6 但含非 hex 字符时, 每对解析失败返回 0.0
        let (r, g, b, _) = parse_hex_color("#zzzzzz");
        assert_eq!(r, 0.0);
        assert_eq!(g, 0.0);
        assert_eq!(b, 0.0);
    }

    #[test]
    fn parse_m_l_z() {
        let mut cmds: Vec<(char, Vec<f32>)> = vec![];
        parse_svg_path("M 0 0 L 10 0 L 10 10 Z", |c, a| {
            cmds.push((c, a.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(cmds.len(), 4);
        assert_eq!(cmds[0].0, 'M');
        assert_eq!(cmds[3].0, 'Z');
    }

    #[test]
    fn parse_relative() {
        let mut cmds: Vec<(char, Vec<f32>)> = vec![];
        parse_svg_path("M 5 5 m 2 2 l 3 3", |c, a| {
            cmds.push((c, a.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(cmds[0].1, vec![5.0, 5.0]);
        assert_eq!(cmds[1].1, vec![7.0, 7.0]);
        assert_eq!(cmds[2].1, vec![10.0, 10.0]);
    }

    #[test]
    fn svg_defs_count_matches() {
        use crate::icons::IconKind;
        use crate::icons_svg_defs::SVG_DEFS;
        assert_eq!(SVG_DEFS.len(), IconKind::ALL.len());
    }

    /// 圆弧（A）应转换为三次贝塞尔曲线而非直线
    #[test]
    fn parse_arc_to_bezier() {
        let mut cmds: Vec<(char, Vec<f32>)> = vec![];
        // 半径 10 的半圆：从 (0,0) 到 (20,0)
        parse_svg_path("M 0 0 A 10 10 0 0 1 20 0", |c, a| {
            cmds.push((c, a.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(cmds[0].0, 'M');
        // 半圆被分为 2 段 ≤90° 的贝塞尔
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[1].0, 'C');
        assert_eq!(cmds[2].0, 'C');
        // 每段贝塞尔终点必须落在圆弧两端连线上（半圆中点 y≈-10 或 +10，端点 x=20）
        let end = &cmds[2].1;
        assert!(
            (end[4] - 20.0).abs() < 0.01,
            "贝塞尔终点 x 应为 20，实际 {}",
            end[4]
        );
        assert!(end[5].abs() < 0.01, "贝塞尔终点 y 应为 0，实际 {}", end[5]);
        // 中间控制点应使曲线鼓出直线（半圆顶点 |y| 接近 10）
        let mid_y = cmds[1].1[5];
        assert!(
            mid_y.abs() > 5.0,
            "半圆中点应明显偏离直线，实际 y={}",
            mid_y
        );
    }

    /// 退化的圆弧（半径为 0 或起终点相同）应退化为直线
    #[test]
    fn parse_degenerate_arc_as_line() {
        let mut cmds: Vec<(char, Vec<f32>)> = vec![];
        parse_svg_path("M 0 0 A 0 5 0 0 1 10 0 A 5 5 0 0 1 10 0", |c, a| {
            cmds.push((c, a.to_vec()));
            Ok(())
        })
        .unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[1].0, 'L');
        assert_eq!(cmds[2].0, 'L');
    }
}
