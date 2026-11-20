use std::collections::BTreeMap;

use pumpkin_data::attributes::Attributes;
use pumpkin_protocol::ser::{NetworkReadExt as _, NetworkReadSliceExt as _, ReadingError};

#[derive(Default)]
pub struct Values(BTreeMap<u8, f64>);

impl Values {
    pub fn update(&mut self, mut bytes: &[u8]) -> Result<(), ReadingError> {
        let properties = count(&mut bytes, 10)?;
        for _ in 0..properties {
            let id = bytes.get_var_int()?.0;
            let mut with_additions = bytes.get_f64_be()?;
            let mut scaled = Vec::new();
            let mut multipliers = Vec::new();
            for _ in 0..count(&mut bytes, 10)? {
                bytes.get_str_borrowed()?;
                let amount = bytes.get_f64_be()?;
                match bytes.get_u8()? {
                    0 => with_additions += amount,
                    1 => scaled.push(amount),
                    2 => multipliers.push(amount),
                    _ => return Err(ReadingError::Message("invalid attribute operation".into())),
                }
            }
            let mut value = with_additions;
            for amount in scaled {
                value += with_additions * amount;
            }
            for amount in multipliers {
                value *= 1.0 + amount;
            }
            if let Ok(id) = u8::try_from(id) {
                self.0.insert(id, value);
            }
        }
        Ok(())
    }

    pub fn get(&self, attribute: &Attributes, default: f64) -> f64 {
        self.0
            .get(&attribute.id)
            .copied()
            .unwrap_or(default)
            .clamp(attribute.min_value, attribute.max_value)
    }
}

fn count(bytes: &mut &[u8], minimum_size: usize) -> Result<usize, ReadingError> {
    let count = bytes.get_var_int()?.0 as usize;
    if count > bytes.len() / minimum_size {
        return Err(ReadingError::Message(
            "invalid attribute list length".into(),
        ));
    }
    Ok(count)
}
