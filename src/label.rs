//! Human-readable text converted to outlines; no installed fonts are required.
use crate::{Anchor, Subpath};
use ttf_parser::{Face, OutlineBuilder};

struct Pen {
    paths: Vec<Subpath>,
    anchors: Vec<Anchor>,
    x: f64,
    baseline: f64,
    scale: f64,
}
impl Pen {
    fn point(&self, x: f32, y: f32) -> [f64; 2] {
        [
            self.x + x as f64 * self.scale,
            self.baseline - y as f64 * self.scale,
        ]
    }
    fn finish(&mut self) {
        if self.anchors.len() > 1 {
            let last = self.anchors.last().unwrap();
            if last.p == self.anchors[0].p {
                self.anchors[0].incoming = last.incoming;
                self.anchors.pop();
            }
            self.paths.push(Subpath {
                closed: true,
                anchors: std::mem::take(&mut self.anchors),
            });
        } else {
            self.anchors.clear();
        }
    }
}
impl OutlineBuilder for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.anchors.push(Anchor::at(self.point(x, y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.anchors.push(Anchor::at(self.point(x, y)));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let cp = self.point(x1, y1);
        let end = self.point(x, y);
        if let Some(prev) = self.anchors.last_mut() {
            let start = prev.p;
            prev.outgoing = Some([
                start[0] + 2.0 * (cp[0] - start[0]) / 3.0,
                start[1] + 2.0 * (cp[1] - start[1]) / 3.0,
            ]);
            let mut anchor = Anchor::at(end);
            anchor.incoming = Some([
                end[0] + 2.0 * (cp[0] - end[0]) / 3.0,
                end[1] + 2.0 * (cp[1] - end[1]) / 3.0,
            ]);
            self.anchors.push(anchor);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let out = self.point(x1, y1);
        let inc = self.point(x2, y2);
        let end = self.point(x, y);
        if let Some(prev) = self.anchors.last_mut() {
            prev.outgoing = Some(out);
        }
        let mut anchor = Anchor::at(end);
        anchor.incoming = Some(inc);
        self.anchors.push(anchor);
    }
    fn close(&mut self) {
        self.finish();
    }
}
pub fn outline(text: &str, center: f64, baseline: f64, size: f64) -> Result<Vec<Subpath>, String> {
    let face = Face::parse(include_bytes!("../assets/RobotoMono.ttf"), 0)
        .map_err(|_| "Cannot read the bundled label font.")?;
    let scale = size / face.units_per_em() as f64;
    let mut width = 0.0;
    for ch in text.chars() {
        let glyph = face
            .glyph_index(ch)
            .ok_or("The label contains an unsupported character.")?;
        width += face.glyph_hor_advance(glyph).unwrap_or(0) as f64 * scale;
    }
    let mut pen = Pen {
        paths: vec![],
        anchors: vec![],
        x: center - width / 2.0,
        baseline,
        scale,
    };
    for ch in text.chars() {
        let glyph = face
            .glyph_index(ch)
            .ok_or("The label contains an unsupported character.")?;
        face.outline_glyph(glyph, &mut pen);
        pen.finish();
        pen.x += face.glyph_hor_advance(glyph).unwrap_or(0) as f64 * scale;
    }
    Ok(pen.paths)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_printable_ascii_is_outlined() {
        let text: String = (32u8..=126).map(char::from).collect();
        let paths = outline(&text, 500.0, 100.0, 10.0).unwrap();
        assert!(paths.len() > 90);
        assert!(
            paths
                .iter()
                .any(|p| p.anchors.iter().any(|a| a.incoming.is_some()))
        );
        assert!(
            paths
                .iter()
                .flat_map(|p| &p.anchors)
                .all(|a| a.p.iter().all(|x| x.is_finite()))
        );
    }
}
