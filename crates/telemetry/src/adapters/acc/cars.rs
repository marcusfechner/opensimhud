//! ACC car model ids (Broadcasting API `carModelType`) → display name and class.
//!
//! The class drives standings grouping and colors. Verify new DLC cars against a real entry
//! list; unknown ids fall back to "Car <id>" in class "GT3".

pub struct Model {
    pub name: &'static str,
    pub class: &'static str,
}

const fn m(name: &'static str, class: &'static str) -> Option<Model> {
    Some(Model { name, class })
}

pub fn model(id: u8) -> Option<Model> {
    match id {
        0 => m("Porsche 991 GT3 R", "GT3"),
        1 => m("Mercedes-AMG GT3", "GT3"),
        2 => m("Ferrari 488 GT3", "GT3"),
        3 => m("Audi R8 LMS", "GT3"),
        4 => m("Lamborghini Huracán GT3", "GT3"),
        5 => m("McLaren 650S GT3", "GT3"),
        6 => m("Nissan GT-R Nismo GT3 2018", "GT3"),
        7 => m("BMW M6 GT3", "GT3"),
        8 => m("Bentley Continental GT3 2018", "GT3"),
        9 => m("Porsche 991 II GT3 Cup", "CUP"),
        10 => m("Nissan GT-R Nismo GT3 2017", "GT3"),
        11 => m("Bentley Continental GT3 2016", "GT3"),
        12 => m("Aston Martin V12 Vantage GT3", "GT3"),
        13 => m("Lamborghini Gallardo R-EX", "GT3"),
        14 => m("Emil Frey Jaguar G3", "GT3"),
        15 => m("Lexus RC F GT3", "GT3"),
        16 => m("Lamborghini Huracán GT3 EVO", "GT3"),
        17 => m("Honda NSX GT3", "GT3"),
        18 => m("Lamborghini Huracán Super Trofeo", "ST"),
        19 => m("Audi R8 LMS EVO", "GT3"),
        20 => m("Aston Martin V8 Vantage GT3", "GT3"),
        21 => m("Honda NSX GT3 EVO", "GT3"),
        22 => m("McLaren 720S GT3", "GT3"),
        23 => m("Porsche 991 II GT3 R", "GT3"),
        24 => m("Ferrari 488 GT3 EVO", "GT3"),
        25 => m("Mercedes-AMG GT3 EVO", "GT3"),
        26 => m("Ferrari 488 Challenge EVO", "CHL"),
        27 => m("BMW M2 CS Racing", "TCX"),
        28 => m("Porsche 992 GT3 Cup", "CUP"),
        29 => m("Lamborghini Huracán Super Trofeo EVO2", "ST"),
        30 => m("BMW M4 GT3", "GT3"),
        31 => m("Audi R8 LMS GT3 EVO II", "GT3"),
        32 => m("Ferrari 296 GT3", "GT3"),
        33 => m("Lamborghini Huracán GT3 EVO2", "GT3"),
        34 => m("Porsche 992 GT3 R", "GT3"),
        35 => m("McLaren 720S GT3 EVO", "GT3"),
        36 => m("Ford Mustang GT3", "GT3"),
        50 => m("Alpine A110 GT4", "GT4"),
        51 => m("Aston Martin Vantage GT4", "GT4"),
        52 => m("Audi R8 LMS GT4", "GT4"),
        53 => m("BMW M4 GT4", "GT4"),
        55 => m("Chevrolet Camaro GT4", "GT4"),
        56 => m("Ginetta G55 GT4", "GT4"),
        57 => m("KTM X-Bow GT4", "GT4"),
        58 => m("Maserati MC GT4", "GT4"),
        59 => m("McLaren 570S GT4", "GT4"),
        60 => m("Mercedes-AMG GT4", "GT4"),
        61 => m("Porsche 718 Cayman GT4", "GT4"),
        80 => m("Audi R8 LMS GT2", "GT2"),
        82 => m("KTM X-Bow GT2", "GT2"),
        83 => m("Maserati MC20 GT2", "GT2"),
        84 => m("Mercedes-AMG GT2", "GT2"),
        85 => m("Porsche 911 GT2 RS CS EVO", "GT2"),
        86 => m("Porsche 935", "GT2"),
        _ => None,
    }
}

/// Classes in the order standings list them.
pub const CLASS_ORDER: [&str; 7] = ["GT3", "GT2", "GT4", "CUP", "ST", "CHL", "TCX"];

/// Driver category letter from the Broadcasting API value.
pub fn category(v: u8) -> &'static str {
    match v {
        3 => "P",
        2 => "G",
        1 => "S",
        0 => "B",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_have_a_listed_class() {
        for id in 0..=255u8 {
            if let Some(m) = model(id) {
                assert!(
                    CLASS_ORDER.contains(&m.class),
                    "{} has class {}",
                    m.name,
                    m.class
                );
            }
        }
        assert_eq!(model(32).unwrap().name, "Ferrari 296 GT3");
        assert_eq!(model(61).unwrap().class, "GT4");
        assert!(model(54).is_none());
    }
}
