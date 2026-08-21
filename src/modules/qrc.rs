use crate::runtime::{expect_string, native};
use crate::value::Value;
use qrcode::QrCode;
use std::collections::BTreeMap;

pub fn module() -> Value {
    let mut fields = BTreeMap::new();

    fields.insert(
        "make".to_string(),
        native("qrc.make", |args| {
            let text = expect_string(
                args.get(0).unwrap_or(&Value::Null),
                "as the QR code content",
            )?;
            let path = expect_string(
                args.get(1).unwrap_or(&Value::Null),
                "as the output file path",
            )?;

            let code = QrCode::new(text.as_bytes())
                .map_err(|e| format!("Could not build QR code: {e}"))?;
            let image = code
                .render::<image::Luma<u8>>()
                .max_dimensions(512, 512)
                .build();
            image
                .save(&path)
                .map_err(|e| format!("Could not save QR code to '{path}': {e}"))?;
            Ok(Value::Null)
        }),
    );

    Value::new_object(fields)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_writes_a_png_file() {
        let Value::Object(obj) = module() else {
            unreachable!()
        };
        let func = obj.borrow().get("make").cloned().unwrap();
        let Value::Native(native) = func else {
            unreachable!()
        };

        let mut path = std::env::temp_dir();
        path.push(format!("em_qrc_test_{}.png", std::process::id()));
        let path_str = path.display().to_string();

        let result = (native.func)(&[
            Value::String("https://example.com".to_string()),
            Value::String(path_str.clone()),
        ]);
        assert!(result.is_ok());
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }
}
