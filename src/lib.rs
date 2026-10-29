//! Offline linear barcode generator for VectorCraft plug-in ABI v1.
mod encoding;
mod label;
use serde::Serialize;
use serde_json::{Value, json};

const MANIFEST: &str = r#"{
 "id":"nl.bram.vectorcraft.barcode","name":"Generate Barcode","version":"1.0.0","kind":"filter","budget":"heavy",
 "author":"breakerh","description":"Add a barcode as editable vector paths, with optional outlined text. Works entirely offline.",
 "params":{
  "symbology":{"type":"choice","options":["Code 128","EAN-13","EAN-8","UPC-A","UPC-E","Code 39","Code 93","ITF","ITF-14","Codabar","ISBN-13"],"default":"Code 128","label":"Barcode type"},
  "data":{"type":"string","default":"EXAMPLE-12345","maxLength":256,"label":"Data","help":"Retail types accept digits only. Enter data without a check digit to calculate it, or with one to validate it. Code 128 accepts printable ASCII; Code 39/93 uppercase letters, digits and standard punctuation."},
  "moduleWidth":{"type":"number","min":0.15,"max":1,"default":0.33,"label":"Narrow bar width","unit":"mm"},
  "barHeight":{"type":"number","min":10,"max":100,"default":23,"label":"Bar height","unit":"mm"},
  "showText":{"type":"bool","default":true,"label":"Show text below barcode"},
  "whiteBackground":{"type":"bool","default":true,"label":"White background"}
 }
}"#;

#[unsafe(no_mangle)]
pub extern "C" fn vc_abi_version() -> u32 {
    1
}
#[unsafe(no_mangle)]
pub extern "C" fn vc_manifest() -> u64 {
    pack(MANIFEST.as_bytes())
}
#[unsafe(no_mangle)]
pub extern "C" fn vc_alloc(size: u32) -> u32 {
    let mut bytes = Vec::<u8>::with_capacity(size.max(1) as usize);
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    ptr as u32
}
fn pack(b: &[u8]) -> u64 {
    ((b.len() as u64) << 32) | b.as_ptr() as u64
}
/// # Safety
/// The host provides valid allocated slices of input_len / params_len bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vc_run(
    input: *const u8,
    input_len: u32,
    params: *const u8,
    params_len: u32,
) -> i64 {
    let (input, params) = unsafe {
        (
            std::slice::from_raw_parts(input, input_len as usize),
            std::slice::from_raw_parts(params, params_len as usize),
        )
    };
    let (Ok(doc), Ok(p)) = (
        serde_json::from_slice::<Value>(input),
        serde_json::from_slice::<Value>(params),
    ) else {
        return -1;
    };
    let result = build(&doc, &p);
    let bytes = match result {
        Ok(out) => Ok(encode_output(&out)),
        Err(error) => serde_json::to_vec(&json!({"error":error})),
    };
    let Ok(bytes) = bytes else {
        return -2;
    };
    pack(bytes.leak()) as i64
}
#[derive(Serialize)]
struct Anchor {
    p: [f64; 2],
    #[serde(rename = "in", skip_serializing_if = "Option::is_none")]
    incoming: Option<[f64; 2]>,
    #[serde(rename = "out", skip_serializing_if = "Option::is_none")]
    outgoing: Option<[f64; 2]>,
}
impl Anchor {
    fn at(p: [f64; 2]) -> Self {
        Self {
            p,
            incoming: None,
            outgoing: None,
        }
    }
}

#[derive(Serialize)]
struct Subpath {
    closed: bool,
    anchors: Vec<Anchor>,
}
#[derive(Serialize)]
struct Geometry {
    subpaths: Vec<Subpath>,
}
#[derive(Serialize)]
struct PathObject {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    path: Geometry,
    #[serde(rename = "fillRule")]
    fill_rule: &'static str,
    fills: Value,
    strokes: Vec<Value>,
    opacity: f64,
}
#[derive(Serialize)]
#[serde(untagged)]
enum Item {
    Existing { id: Value },
    New(PathObject),
}
#[derive(Serialize)]
struct Output {
    objects: Vec<Item>,
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> Subpath {
    Subpath {
        closed: true,
        anchors: vec![
            Anchor::at([x, y]),
            Anchor::at([x + w, y]),
            Anchor::at([x + w, y + h]),
            Anchor::at([x, y + h]),
        ],
    }
}
fn object(name: &str, subpaths: Vec<Subpath>, k: f64) -> Item {
    Item::New(PathObject {
        kind: "path",
        name: name.into(),
        path: Geometry { subpaths },
        fill_rule: "nonzero",
        fills: json!([{"type":"solid","color":{"model":"gray","k":k}}]),
        strokes: vec![],
        opacity: 1.0,
    })
}
fn build(doc: &Value, p: &Value) -> Result<Output, String> {
    let kind = p["symbology"].as_str().unwrap_or("Code 128");
    let data = p["data"].as_str().unwrap_or("");
    let encoded = encoding::encode(kind, data)?;
    let module_mm = p["moduleWidth"].as_f64().unwrap_or(0.33);
    let height_mm = p["barHeight"].as_f64().unwrap_or(23.0);
    if !module_mm.is_finite() || !(0.15..=1.0).contains(&module_mm) {
        return Err("Choose a narrow bar width between 0.15 and 1 mm.".into());
    }
    if !height_mm.is_finite() || !(10.0..=100.0).contains(&height_mm) {
        return Err("Choose a bar height between 10 and 100 mm.".into());
    }
    let unit = module_mm * 72.0 / 25.4;
    let height = height_mm * 72.0 / 25.4;
    // Use generous horizontal quiet zones for every symbology (at least 11 modules / 6.5 mm).
    let quiet = (11.0 * unit).max(6.5 * 72.0 / 25.4);
    let width = encoded.bits.len() as f64 * unit + 2.0 * quiet;
    let vertical = 2.0 * 72.0 / 25.4;
    let show_text = p["showText"].as_bool().unwrap_or(true);
    let bearer = if kind == "ITF-14" { 5.0 * unit } else { 0.0 };
    let total_height = height + 2.0 * bearer + 2.0 * vertical + if show_text { 15.0 } else { 0.0 };
    let board = p["_context"]["artboard"].as_array();
    let (x, y) = if let Some(b) = board.filter(|b| b.len() == 4) {
        (
            (b[0].as_f64().unwrap_or(0.0) + b[2].as_f64().unwrap_or(612.0) - width) / 2.0,
            (b[1].as_f64().unwrap_or(0.0) + b[3].as_f64().unwrap_or(792.0) - total_height) / 2.0,
        )
    } else {
        (0.0, 0.0)
    };
    let mut objects: Vec<Item> = doc["objects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o.get("id").map(|id| Item::Existing { id: id.clone() }))
        .collect();
    if p["whiteBackground"].as_bool().unwrap_or(true) {
        objects.push(object(
            "Barcode - white quiet zone",
            vec![rect(x, y, width, total_height)],
            0.0,
        ));
    }
    let bar_x = x + quiet;
    let bar_y = y + vertical + bearer;
    let mut paths = Vec::new();
    let mut i = 0;
    while i < encoded.bits.len() {
        if encoded.bits[i] == 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < encoded.bits.len() && encoded.bits[i] == 1 {
            i += 1;
        }
        paths.push(rect(
            bar_x + start as f64 * unit,
            bar_y,
            (i - start) as f64 * unit,
            height
                + if show_text && retail_guard(kind, start) {
                    5.0 * unit
                } else {
                    0.0
                },
        ));
    }
    if bearer > 0.0 {
        let w = encoded.bits.len() as f64 * unit;
        paths.push(rect(bar_x, bar_y - bearer, w, bearer));
        paths.push(rect(bar_x, bar_y + height, w, bearer));
    }
    objects.push(object(
        &format!("Barcode - {kind} - {}", encoded.text),
        paths,
        1.0,
    ));
    if show_text {
        let baseline = bar_y + height + bearer + 12.0;
        let text_paths = match retail_label(kind, &encoded.text, bar_x, baseline, unit)? {
            Some(paths) => paths,
            None => label::outline(&encoded.text, x + width / 2.0, baseline, 10.0)?,
        };
        objects.push(object("Barcode - outlined text", text_paths, 1.0));
    }
    Ok(Output { objects })
}

fn retail_guard(kind: &str, index: usize) -> bool {
    match kind {
        "EAN-13" | "ISBN-13" => index < 3 || (45..50).contains(&index) || index >= 92,
        "UPC-A" => index < 10 || (45..50).contains(&index) || index >= 85,
        "EAN-8" => index < 3 || (31..36).contains(&index) || index >= 64,
        "UPC-E" => index < 3 || index >= 45,
        _ => false,
    }
}
fn retail_label(
    kind: &str,
    text: &str,
    x: f64,
    baseline: f64,
    unit: f64,
) -> Result<Option<Vec<Subpath>>, String> {
    let positions: Vec<f64> = match kind {
        "EAN-13" | "ISBN-13" => std::iter::once(-3.5)
            .chain((0..6).map(|i| 6.5 + i as f64 * 7.0))
            .chain((0..6).map(|i| 53.5 + i as f64 * 7.0))
            .collect(),
        "EAN-8" => (0..4)
            .map(|i| 6.5 + i as f64 * 7.0)
            .chain((0..4).map(|i| 39.5 + i as f64 * 7.0))
            .collect(),
        "UPC-A" => std::iter::once(-3.5)
            .chain((0..5).map(|i| 13.5 + i as f64 * 7.0))
            .chain((0..5).map(|i| 53.5 + i as f64 * 7.0))
            .chain(std::iter::once(98.5))
            .collect(),
        "UPC-E" => std::iter::once(-3.5)
            .chain((0..6).map(|i| 6.5 + i as f64 * 7.0))
            .chain(std::iter::once(54.5))
            .collect(),
        _ => return Ok(None),
    };
    let mut paths = vec![];
    // Keep each digit within its seven-module slot, even at a small module width.
    let size = 10.0_f64.min(unit * 10.0);
    for (ch, position) in text.chars().zip(positions) {
        paths.extend(label::outline(
            &ch.to_string(),
            x + position * unit,
            baseline,
            size,
        )?);
    }
    Ok(Some(paths))
}

// Fixed decimal coordinates avoid expensive general floating-point formatting in the interpreter.
// Precision is 0.0001 pt, far below output/printing precision.
fn encode_output(out: &Output) -> Vec<u8> {
    use std::fmt::Write;
    fn coord(s: &mut String, v: f64) {
        let n = (v * 10000.0).round() as i64;
        if n < 0 {
            s.push('-');
        }
        let n = n.unsigned_abs();
        write!(s, "{}.{:04}", n / 10000, n % 10000).unwrap();
    }
    let mut s = String::from("{\"objects\":[");
    for (i, item) in out.objects.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        match item {
            Item::Existing { id } => {
                write!(s, "{{\"id\":{}}}", id).unwrap();
            }
            Item::New(o) => {
                write!(s,"{{\"type\":\"path\",\"name\":{},\"fillRule\":\"nonzero\",\"fills\":{},\"strokes\":[],\"opacity\":1,\"path\":{{\"subpaths\":[",serde_json::to_string(&o.name).unwrap(),o.fills).unwrap();
                for (j, p) in o.path.subpaths.iter().enumerate() {
                    if j > 0 {
                        s.push(',');
                    }
                    s.push_str("{\"closed\":true,\"anchors\":[");
                    for (k, a) in p.anchors.iter().enumerate() {
                        if k > 0 {
                            s.push(',');
                        }
                        s.push_str("{\"p\":[");
                        coord(&mut s, a.p[0]);
                        s.push(',');
                        coord(&mut s, a.p[1]);
                        s.push(']');
                        for (key, point) in [("in", a.incoming), ("out", a.outgoing)] {
                            if let Some(point) = point {
                                write!(s, ",\"{key}\":[").unwrap();
                                coord(&mut s, point[0]);
                                s.push(',');
                                coord(&mut s, point[1]);
                                s.push(']');
                            }
                        }
                        s.push('}');
                    }
                    s.push_str("]}");
                }
                s.push_str("]}}");
            }
        }
    }
    s.push_str("]}");
    s.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserve_objects_and_dimensions() {
        let doc = json!({"objects":[{"id":42,"name":"Keep me"}]});
        let p = json!({"data":"EXAMPLE-123","moduleWidth":0.33,"barHeight":23,"_context":{"artboard":[100,200,700,1000]}});
        let out = build(&doc, &p).unwrap();
        let value: Value = serde_json::from_slice(&encode_output(&out)).unwrap();
        assert_eq!(value["objects"][0], json!({"id":42}));
        assert_eq!(value["objects"].as_array().unwrap().len(), 4);
        let a = &value["objects"][2]["path"]["subpaths"][0]["anchors"];
        let h = a[2]["p"][1].as_f64().unwrap() - a[0]["p"][1].as_f64().unwrap();
        assert!((h - 23.0 * 72.0 / 25.4).abs() < 0.0002);
    }
    #[test]
    fn transparent_without_text() {
        let out = build(
            &json!({"objects":[]}),
            &json!({"data":"123456","whiteBackground":false,"showText":false}),
        )
        .unwrap();
        assert_eq!(out.objects.len(), 1);
    }
    #[test]
    fn invalid_dimensions_and_data() {
        for p in [
            json!({"data":""}),
            json!({"data":"test","moduleWidth":0.01}),
            json!({"data":"test","barHeight":0}),
            json!({"symbology":"unknown","data":"1234"}),
        ] {
            assert!(build(&json!({"objects":[]}), &p).is_err());
        }
    }
    #[test]
    fn manifest_is_valid() {
        let m: Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(
            m["params"]["symbology"]["options"]
                .as_array()
                .unwrap()
                .len(),
            11
        );
    }
}
