//! Font resources are shared across text-service handles, never across backends.
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use voidui_gpui_wgpu::{ParleyTextSystem, TextSystem};

fn bytes() -> Vec<Cow<'static, [u8]>> {
    vec![Cow::Borrowed(include_bytes!(
        "fonts/IBMPlexSans-Regular.ttf"
    ))]
}

#[test]
fn immutable_resources_load_once_per_backend_even_with_concurrent_handles() {
    let backend = Arc::new(ParleyTextSystem::new_without_system_fonts("IBM Plex Sans"));
    let calls = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let system = TextSystem::new(backend.clone());
            let calls = &calls;
            scope.spawn(move || {
                system
                    .add_fonts_once("test/plex", || {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Ok(bytes())
                    })
                    .unwrap();
            });
        }
    });
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let system = TextSystem::new(backend);
    assert_eq!(system.font_revision(), 1);
    system
        .add_fonts_once("test/plex", || panic!("cached loader must not run"))
        .unwrap();
    assert_eq!(system.font_revision(), 1);
    let independent = TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts(
        "IBM Plex Sans",
    )));
    independent
        .add_fonts_once("test/plex", || {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(bytes())
        })
        .unwrap();
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[test]
fn failed_resource_registration_can_retry_without_changing_revision() {
    let system = TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts(
        "IBM Plex Sans",
    )));
    assert!(
        system
            .add_fonts_once("test/retry", || Ok(vec![Cow::Borrowed(b"invalid font")]))
            .is_err()
    );
    assert_eq!(system.font_revision(), 0);
    assert!(
        system
            .add_fonts_once("test/retry", || Ok(Vec::new()))
            .is_err()
    );
    assert_eq!(system.font_revision(), 0);
    system.add_fonts_once("test/retry", || Ok(bytes())).unwrap();
    assert_eq!(system.font_revision(), 1);
}

#[test]
fn named_resources_do_not_replace_the_default_ui_family() {
    use voidui_gpui_wgpu::{TextRun, font};
    let system = TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts(
        "unused",
    )));
    system.add_fonts(bytes()).unwrap();
    let shape = || {
        system
            .shape_paragraph(
                "Hello".into(),
                &[TextRun {
                    len: 5,
                    font: font("sans-serif"),
                    ..Default::default()
                }],
                16.0,
                20.0,
                None,
                None,
            )
            .unwrap()
    };
    let before = shape();
    system
        .add_fonts_once("test/ahem", || {
            Ok(vec![Cow::Borrowed(include_bytes!("fonts/Ahem.ttf"))])
        })
        .unwrap();
    let after = shape();
    let before_font = before
        .layout()
        .get(0)
        .unwrap()
        .runs()
        .next()
        .unwrap()
        .font()
        .clone();
    let after_font = after
        .layout()
        .get(0)
        .unwrap()
        .runs()
        .next()
        .unwrap()
        .font()
        .clone();
    assert_eq!(before_font.data.id(), after_font.data.id());
    assert_eq!(before.width(), after.width());
}

#[test]
fn declared_face_metadata_controls_matching_instead_of_font_file_flags() {
    use voidui_gpui_wgpu::{Font, FontStyle, TextRun, font};
    let system = TextSystem::new(Arc::new(ParleyTextSystem::new_without_system_fonts(
        "unused",
    )));
    // Both fixtures are upright fonts. Assign them different declared styles
    // under one family to test metadata overrides without vendor-specific data.
    let regular = Font {
        family: "DeclaredFamily".into(),
        ..font("unused")
    };
    let italic = Font {
        style: FontStyle::Italic,
        ..regular.clone()
    };
    let a = include_bytes!("fonts/IBMPlexSans-Regular.ttf").as_slice();
    let b = include_bytes!("fonts/Ahem.ttf").as_slice();
    system
        .add_font_face_once("declared/italic", &italic, || Ok(Cow::Borrowed(b)))
        .unwrap();
    system
        .add_font_face_once("declared/regular", &regular, || Ok(Cow::Borrowed(a)))
        .unwrap();
    let revision = system.font_revision();
    system
        .add_font_face_once("declared/italic", &italic, || {
            panic!("registered face must be reused")
        })
        .unwrap();
    assert_eq!(system.font_revision(), revision);
    for (face, data) in [(regular, a), (italic, b)] {
        let paragraph = system
            .shape_paragraph(
                "A".into(),
                &[TextRun {
                    len: 1,
                    font: face,
                    ..Default::default()
                }],
                16.0,
                20.0,
                None,
                None,
            )
            .unwrap();
        for line in paragraph.layout().lines() {
            for run in line.runs() {
                assert!(run.font().data.as_ref() == data);
                assert!(run.synthesis().skew().is_none());
            }
        }
    }
}
