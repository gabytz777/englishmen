//! A tiny framebuffer module for Englishmen.
//!
//! It deliberately knows nothing about 3D: it only stores RGB pixels and can
//! write them to a PNG. All of the projection, shading and rasterisation is
//! done in Englishmen source code by `renderer/render.enm`.

use crate::runtime::{expect_number, expect_string, native};
use crate::value::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

struct CanvasData {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

impl CanvasData {
    fn new(width: usize, height: usize) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Canvas dimensions must be greater than zero".to_string());
        }
        if width > 4096 || height > 4096 {
            return Err("Canvas dimensions must not exceed 4096".to_string());
        }
        Ok(Self {
            width,
            height,
            pixels: vec![0; width * height * 3],
        })
    }

    fn set(&mut self, x: i64, y: i64, r: u8, g: u8, b: u8) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let index = ((y as usize * self.width) + x as usize) * 3;
        self.pixels[index] = r;
        self.pixels[index + 1] = g;
        self.pixels[index + 2] = b;
    }

    fn rgb_at(&self, x: i64, y: i64) -> [u8; 3] {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return [0, 0, 0];
        }
        let index = ((y as usize * self.width) + x as usize) * 3;
        [self.pixels[index], self.pixels[index + 1], self.pixels[index + 2]]
    }
}

fn num(args: &[Value], index: usize) -> Result<f64, String> {
    expect_number(args.get(index).unwrap_or(&Value::Null), "as a number")
}

fn colour(args: &[Value], offset: usize) -> Result<[u8; 3], String> {
    let mut out = [0u8; 3];
    for (i, channel) in out.iter_mut().enumerate() {
        let value = num(args, offset + i)?;
        *channel = if value <= 0.0 {
            0
        } else if value >= 255.0 {
            255
        } else {
            value.round() as u8
        };
    }
    Ok(out)
}

fn glyph(c: char) -> Option<&'static [u8; 7]> {
    const FONT: &[(char, [u8; 7])] = &[
        (' ', [0, 0, 0, 0, 0, 0, 0]),
        ('A', [14, 17, 17, 31, 17, 17, 17]),
        ('B', [30, 17, 17, 30, 17, 17, 30]),
        ('C', [14, 17, 16, 16, 16, 17, 14]),
        ('D', [30, 17, 17, 17, 17, 17, 30]),
        ('E', [31, 16, 16, 30, 16, 16, 31]),
        ('F', [31, 16, 16, 30, 16, 16, 16]),
        ('G', [14, 17, 16, 23, 17, 17, 14]),
        ('H', [17, 17, 17, 31, 17, 17, 17]),
        ('I', [14, 4, 4, 4, 4, 4, 14]),
        ('J', [7, 2, 2, 2, 2, 18, 12]),
        ('K', [17, 18, 20, 24, 20, 18, 17]),
        ('L', [16, 16, 16, 16, 16, 16, 31]),
        ('M', [17, 27, 21, 21, 17, 17, 17]),
        ('N', [17, 25, 21, 19, 17, 17, 17]),
        ('O', [14, 17, 17, 17, 17, 17, 14]),
        ('P', [30, 17, 17, 30, 16, 16, 16]),
        ('Q', [14, 17, 17, 17, 21, 18, 13]),
        ('R', [30, 17, 17, 30, 20, 18, 17]),
        ('S', [15, 16, 16, 14, 1, 1, 30]),
        ('T', [31, 4, 4, 4, 4, 4, 4]),
        ('U', [17, 17, 17, 17, 17, 17, 14]),
        ('V', [17, 17, 17, 17, 17, 10, 4]),
        ('W', [17, 17, 17, 21, 21, 27, 17]),
        ('X', [17, 17, 10, 4, 10, 17, 17]),
        ('Y', [17, 17, 10, 4, 4, 4, 4]),
        ('Z', [31, 1, 2, 4, 8, 16, 31]),
        ('0', [14, 17, 19, 21, 25, 17, 14]),
        ('1', [4, 12, 4, 4, 4, 4, 14]),
        ('2', [14, 17, 1, 2, 4, 8, 31]),
        ('3', [30, 1, 1, 14, 1, 1, 30]),
        ('4', [2, 6, 10, 18, 31, 2, 2]),
        ('5', [31, 16, 16, 30, 1, 1, 30]),
        ('6', [14, 16, 16, 30, 17, 17, 14]),
        ('7', [31, 1, 2, 4, 8, 8, 8]),
        ('8', [14, 17, 17, 14, 17, 17, 14]),
        ('9', [14, 17, 17, 15, 1, 1, 14]),
        ('.', [0, 0, 0, 0, 0, 12, 12]),
        ('-', [0, 0, 0, 31, 0, 0, 0]),
        (':', [0, 12, 12, 0, 12, 12, 0]),
        ('/', [1, 1, 2, 4, 8, 16, 16]),
    ];
    let upper = c.to_ascii_uppercase();
    FONT.iter().find(|(key, _)| *key == upper).map(|(_, rows)| rows)
}

fn canvas_object(data: Rc<RefCell<CanvasData>>) -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "width".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.width", move |_| {
                Ok(Value::Number(data.borrow().width as f64))
            })
        },
    );

    fields.insert(
        "height".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.height", move |_| {
                Ok(Value::Number(data.borrow().height as f64))
            })
        },
    );

    fields.insert(
        "set".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.set", move |args| {
                let x = num(args, 0)?.round() as i64;
                let y = num(args, 1)?.round() as i64;
                let [r, g, b] = colour(args, 2)?;
                data.borrow_mut().set(x, y, r, g, b);
                Ok(Value::Null)
            })
        },
    );

    fields.insert(
        "get".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.get", move |args| {
                let x = num(args, 0)?.round() as i64;
                let y = num(args, 1)?.round() as i64;
                let [r, g, b] = data.borrow().rgb_at(x, y);
                Ok(Value::new_array(vec![
                    Value::Number(r as f64),
                    Value::Number(g as f64),
                    Value::Number(b as f64),
                ]))
            })
        },
    );

    fields.insert(
        "clear".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.clear", move |args| {
                let [r, g, b] = colour(args, 0)?;
                let mut canvas = data.borrow_mut();
                for chunk in canvas.pixels.chunks_exact_mut(3) {
                    chunk[0] = r;
                    chunk[1] = g;
                    chunk[2] = b;
                }
                Ok(Value::Null)
            })
        },
    );

    fields.insert(
        "text".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.text", move |args| {
                let x = num(args, 0)?.round() as i64;
                let y = num(args, 1)?.round() as i64;
                let message = expect_string(args.get(2).unwrap_or(&Value::Null), "as text")?;
                let [r, g, b] = colour(args, 3)?;
                let scale = num(args, 6)?.round() as i64;
                let scale = scale.clamp(1, 16);
                let mut canvas = data.borrow_mut();
                let mut cursor = x;
                for c in message.chars() {
                    if let Some(rows) = glyph(c) {
                        for (row, bits) in rows.iter().enumerate() {
                            for col in 0..5 {
                                if bits & (1 << (4 - col)) != 0 {
                                    for sy in 0..scale {
                                        for sx in 0..scale {
                                            canvas.set(
                                                cursor + col as i64 * scale + sx,
                                                y + row as i64 * scale + sy,
                                                r,
                                                g,
                                                b,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    cursor += 6 * scale;
                }
                Ok(Value::Null)
            })
        },
    );

    fields.insert(
        "save".to_string(),
        {
            let data = Rc::clone(&data);
            native("gfx.canvas.save", move |args| {
                let path = expect_string(args.get(0).unwrap_or(&Value::Null), "as the output path")?;
                let canvas = data.borrow();
                let image = image::RgbImage::from_raw(
                    canvas.width as u32,
                    canvas.height as u32,
                    canvas.pixels.clone(),
                )
                .ok_or_else(|| "Could not build the image from the canvas".to_string())?;
                image
                    .save(&path)
                    .map_err(|e| format!("Could not save image to '{path}': {e}"))?;
                Ok(Value::Null)
            })
        },
    );

    Value::new_object(fields)
}

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "canvas".to_string(),
        native("gfx.canvas", |args| {
            let width = num(args, 0)?.round() as i64;
            let height = num(args, 1)?.round() as i64;
            if width <= 0 || height <= 0 {
                return Err("Canvas dimensions must be greater than zero".to_string());
            }
            let data = Rc::new(RefCell::new(CanvasData::new(width as usize, height as usize)?));
            Ok(canvas_object(data))
        }),
    );

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(obj: &Value, name: &str, args: &[Value]) -> Value {
        let Value::Object(fields) = obj else { unreachable!() };
        let Value::Native(func) = fields.borrow().get(name).cloned().unwrap() else {
            unreachable!()
        };
        (func.func)(args).unwrap()
    }

    #[test]
    fn set_get_and_save() {
        let canvas = call(&module(), "canvas", &[Value::Number(8.0), Value::Number(4.0)]);
        call(
            &canvas,
            "set",
            &[
                Value::Number(2.0),
                Value::Number(1.0),
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0),
            ],
        );
        let pixel = call(
            &canvas,
            "get",
            &[Value::Number(2.0), Value::Number(1.0)],
        );
        let Value::Array(rgb) = pixel else { unreachable!() };
        assert_eq!(
            rgb.borrow()
                .iter()
                .map(|v| match v {
                    Value::Number(n) => *n as u8,
                    _ => 0,
                })
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );

        let path = std::env::temp_dir().join(format!("em_gfx_test_{}.png", std::process::id()));
        call(&canvas, "save", &[Value::String(path.display().to_string())]);
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }
}
