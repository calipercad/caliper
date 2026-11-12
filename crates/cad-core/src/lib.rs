//! # cad-core
//!
//! The engine behind `Caliper`: a terminal-native, AI-assisted parametric
//! CAD toolkit. Pure `std` — no external crates — so it builds offline and
//! stays auditable.
//!
//! Layers:
//! - [`math`] / [`bbox`] — linear algebra and bounds
//! - [`geom2d`] / [`geom3d`] — the geometry kernel (shapes, meshes, extrude)
//! - [`skills`] — parametric generators (the extensible design library)
//! - [`export`] — SVG/DXF/G-code/STL/OBJ writers
//! - [`render`] — braille terminal previews
//! - [`ai`] — natural-language → script bridge (via `curl`/`jq`)
//! - [`script`] — the command language and [`script::Interpreter`]
//!
//! ```no_run
//! use cad_core::script::Interpreter;
//! let mut it = Interpreter::new();
//! it.exec_line("skill gear teeth=20 module=2 thickness=5").unwrap();
//! println!("{}", it.exec_line("render").unwrap());
//! ```

pub mod ai;
pub mod args;
pub mod bbox;
pub mod error;
pub mod export;
pub mod geom2d;
pub mod geom3d;
pub mod import;
pub mod math;
pub mod render;
pub mod scene;
pub mod script;
pub mod skills;

pub use error::{CadError, Result};
pub use scene::{Scene, Units};
pub use script::Interpreter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_2d() {
        let mut it = Interpreter::new();
        it.exec_line("circle 10 name=c1").unwrap();
        it.exec_line("rect 20 10 name=r1").unwrap();
        let art = it.exec_line("render mode=2d").unwrap();
        assert!(art.contains('\n'));
        assert_eq!(it.scene.entities.len(), 2);
    }

    #[test]
    fn extrude_and_mesh() {
        let mut it = Interpreter::new();
        it.exec_line("circle 5 name=c").unwrap();
        it.exec_line("extrude 10 target=c name=solid").unwrap();
        let e = it.scene.get("solid").unwrap();
        match &e.geom {
            scene::Geometry::Mesh(m) => assert!(m.triangle_count() > 0),
            _ => panic!("expected mesh"),
        }
    }

    #[test]
    fn gear_skill_runs() {
        let mut it = Interpreter::new();
        let msg = it.exec_line("skill gear teeth=12 module=2").unwrap();
        assert!(msg.contains("gear"));
        assert_eq!(it.scene.entities.len(), 1);
    }

    #[test]
    fn export_svg_string() {
        let mut it = Interpreter::new();
        it.exec_line("rect 10 10 name=r").unwrap();
        let shapes: Vec<export::Item> = it
            .scene
            .entities
            .iter()
            .filter_map(|e| match &e.geom {
                scene::Geometry::Shape(s) => Some(export::Item::new(e.name.as_str(), s)),
                _ => None,
            })
            .collect();
        let svg = export::svg::to_svg(&shapes, 1.0);
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<path"));
    }

    #[test]
    fn tokenizer_handles_quotes_and_comments() {
        let t = script::tokenize("ai \"make a gear\" # comment");
        assert_eq!(t, vec!["ai", "make a gear"]);
        assert!(script::tokenize("   # only comment").is_empty());
    }

    #[test]
    fn path_closed_flag_is_not_a_coordinate() {
        // A trailing bare `closed` must be treated as a flag, not miscounted as
        // a coordinate, and the resulting contour must be closed.
        let mut it = Interpreter::new();
        it.exec_line("path 0 0  10 0  10 5  0 5  closed name=p")
            .unwrap();
        match &it.scene.get("p").unwrap().geom {
            scene::Geometry::Shape(s) => {
                assert_eq!(s.contours[0].points.len(), 4);
                assert!(s.contours[0].closed, "closed flag should close the contour");
            }
            _ => panic!("expected shape"),
        }
        // An open path (no flag) stays open.
        it.exec_line("path 0 0  10 0  10 5 name=q").unwrap();
        match &it.scene.get("q").unwrap().geom {
            scene::Geometry::Shape(s) => assert!(!s.contours[0].closed),
            _ => panic!("expected shape"),
        }
    }

    #[test]
    fn script_skill_records_only_the_invocation() {
        // A script-skill expands into several sub-commands, but `save` must
        // record only the single deterministic `skill ...` line — never the
        // expansion — so replaying a saved file rebuilds the scene exactly once.
        let dir = std::env::temp_dir().join(format!(
            "cad-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Two-line body: if the expansion leaked into history it would show up.
        std::fs::write(
            dir.join("plate.cad"),
            "rect ${w:50} ${h:30} name=${name:plate}\ncircle ${r:5} name=${name:plate}_hole\n",
        )
        .unwrap();

        let mut it = Interpreter::new();
        it.user_skill_dir = Some(dir.clone());
        it.exec_line("units mm").unwrap();
        it.exec_line("skill plate w=80 h=40 r=6 name=lid").unwrap();

        // Expansion built two entities...
        assert_eq!(it.scene.entities.len(), 2);
        assert!(it.scene.get("lid").is_some());
        assert!(it.scene.get("lid_hole").is_some());
        // ...but history holds only the units + skill lines, not the sub-commands.
        assert_eq!(
            it.history,
            vec![
                "units mm".to_string(),
                "skill plate w=80 h=40 r=6 name=lid".to_string(),
            ]
        );

        // Round-trip through save: exactly one units header, no duplicate, and
        // the skill line preserved. Then replaying rebuilds the same 2 entities.
        let out = dir.join("out.cad");
        it.exec_line(&format!("save {}", out.display())).unwrap();
        let saved = std::fs::read_to_string(&out).unwrap();
        assert_eq!(saved.matches("units mm").count(), 1);
        assert!(saved.contains("skill plate w=80 h=40 r=6 name=lid"));

        let mut replay = Interpreter::new();
        replay.user_skill_dir = Some(dir.clone());
        replay.exec_script(&saved).unwrap();
        assert_eq!(replay.scene.entities.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
