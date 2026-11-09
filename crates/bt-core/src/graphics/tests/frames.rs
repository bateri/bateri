//! Animation frames: the edit of a stored image's pixels, composition, deletion
//! and the feature query that tells a program what works.

use super::*;

/// The answer to a frame command that was given `r=` and succeeded.
fn frame_ok(id: u32, frame: u32) -> Vec<u8> {
    format!("\x1b_Gi={id},r={frame};OK\x1b\\").into_bytes()
}

/// Every frame of an image on the main screen, whole.
fn frames_of(rig: &Rig, id: u32) -> Vec<Vec<u8>> {
    rig.hub.with_images(|images| {
        images
            .main()
            .get(id)
            .map(|image| {
                image
                    .frames
                    .iter()
                    .map(|frame| frame.pixels.as_ref().clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn gaps_of(rig: &Rig, id: u32) -> Vec<u32> {
    rig.hub.with_images(|images| {
        images
            .main()
            .get(id)
            .map(|image| image.frames.iter().map(|frame| frame.gap).collect())
            .unwrap_or_default()
    })
}

fn revision_of(rig: &Rig, id: u32) -> u64 {
    rig.hub
        .with_images(|images| images.main().get(id).map_or(0, |image| image.revision))
}

/// The pictures the grid shows, as a frame draws them.
pub(super) fn pictures_of(rig: &Rig) -> Pictures {
    let mut pictures = Pictures::default();
    rig.hub.with_images(|images| {
        images.collect(
            false,
            (10, 20),
            80,
            Window {
                first: 0,
                last: 23,
                shift: 0,
            },
            None,
            &mut pictures,
        );
    });
    pictures
}

/// A 4 × 3 image sent as `i=7`, shown at the cursor and left there.
fn shown_image(rig: &mut Rig, pixels: &[u8]) {
    rig.run(&wire("a=T,f=32,s=4,v=3,i=7,C=1", pixels));
}

fn answered(rig: &mut Rig, control: &str, payload: &[u8]) -> String {
    let (_, answers) = rig.run(&wire(control, payload));
    error_of(&answers)
}

// ─── editing a frame ────────────────────────────────────────────────────

#[test]
fn an_edit_of_the_first_frame_changes_only_its_rectangle() {
    let mut rig = Rig::new();
    let base = rgba(4, 3, 1);
    shown_image(&mut rig, &base);
    let before = placed(&rig);
    let patch = [9u8; 8];
    let (motions, answers) = rig.run(&wire("a=f,i=7,r=1,x=1,y=1,s=2,v=1,X=1", &patch));
    assert_eq!(
        motions,
        [STAYS],
        "a frame is not a placement: nothing moves"
    );
    assert_eq!(answers, frame_ok(7, 1));
    let mut expected = base.clone();
    expected[(4 + 1) * 4..(4 + 3) * 4].copy_from_slice(&patch);
    assert_eq!(rig.pixels(7), Some(expected));
    assert_eq!(placed(&rig), before, "the placement stays where it was");
    assert_eq!(frames_of(&rig, 7).len(), 1);
}

#[test]
fn an_edit_that_comes_in_chunks_is_one_edit() {
    let mut rig = Rig::new();
    let base = rgba(4, 3, 1);
    shown_image(&mut rig, &base);
    // Eight bytes of 9 are "CQkJCQkJCQk=": two chunks, each naming `a=f`.
    let (motions, answers) = rig.run(
        &[
            ESC_G,
            b"a=f,i=7,r=1,x=1,y=1,s=2,v=1,X=1,m=1;CQkJCQkJ",
            ST,
            ESC_G,
            b"a=f,m=0;CQk=",
            ST,
        ]
        .concat(),
    );
    assert_eq!(motions, [STAYS], "one record for the two chunks");
    assert_eq!(answers, frame_ok(7, 1));
    let mut expected = base;
    expected[(4 + 1) * 4..(4 + 3) * 4].copy_from_slice(&[9; 8]);
    assert_eq!(rig.pixels(7), Some(expected));
}

#[test]
fn a_frame_that_names_no_size_is_as_big_as_the_image() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let whole = rgba(4, 3, 50);
    assert_eq!(
        answered(&mut rig, "a=f,i=7,r=1,X=1", &whole),
        error_of(&frame_ok(7, 1))
    );
    assert_eq!(rig.pixels(7), Some(whole));
}

#[test]
fn a_translucent_frame_is_blended_and_the_overwrite_key_lays_it_as_it_is() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=7", &[100, 0, 0, 255]));
    rig.run(&wire("a=f,i=7,r=1", &[0, 200, 0, 128]));
    assert_eq!(
        rig.pixels(7),
        Some(vec![50, 100, 0, 255]),
        "half green on red"
    );
    rig.run(&wire("a=f,i=7,r=1,X=1", &[0, 200, 0, 128]));
    assert_eq!(rig.pixels(7), Some(vec![0, 200, 0, 128]), "replaced");
}

#[test]
fn rgb_frames_are_opaque_and_a_png_frame_has_its_own_size() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &[1, 2, 3, 4, 5, 6, 7, 8]));
    rig.run(&wire("a=f,i=7,r=1,x=1,s=1,v=1,f=24", &[9, 8, 7]));
    assert_eq!(rig.pixels(7), Some(vec![1, 2, 3, 4, 9, 8, 7, 255]));
    let png = png_file(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        None,
        None,
        &[60, 61, 62, 255],
    );
    rig.run(&wire("a=f,i=7,r=1,f=100", &png));
    assert_eq!(
        rig.pixels(7),
        Some(vec![60, 61, 62, 255, 9, 8, 7, 255]),
        "the PNG is a rectangle at the corner"
    );
}

// ─── a new frame ────────────────────────────────────────────────────────

#[test]
fn a_new_frame_starts_from_the_frame_c_names_or_from_a_color() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &[1, 2, 3, 255, 4, 5, 6, 255]));
    let answers = answered(&mut rig, "a=f,i=7,c=1,x=1,s=1,v=1,X=1", &[10, 20, 30, 255]);
    assert_eq!(answers, error_of(b"\x1b_Gi=7;OK\x1b\\"), "r was not given");
    rig.run(&wire(
        "a=f,i=7,Y=4278190335,s=1,v=1,X=1,z=25",
        &[9, 9, 9, 9],
    ));
    rig.run(&wire("a=f,i=7,c=2,z=-1,s=1,v=1,X=1", &[7, 7, 7, 255]));
    assert_eq!(
        frames_of(&rig, 7),
        [
            vec![1, 2, 3, 255, 4, 5, 6, 255],
            vec![1, 2, 3, 255, 10, 20, 30, 255],
            // The color is 0xff0000ff: opaque red, laid over by the pixel.
            vec![9, 9, 9, 9, 255, 0, 0, 255],
            vec![7, 7, 7, 255, 10, 20, 30, 255],
        ]
    );
    assert_eq!(
        gaps_of(&rig, 7),
        [0, 40, 25, 0],
        "the root has none, a new frame 40 ms, a given one its own, a negative one passes"
    );
}

#[test]
fn a_new_frame_with_no_base_and_no_color_starts_transparent() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &[1, 2, 3, 255, 4, 5, 6, 255]));
    rig.run(&wire("a=f,i=7,x=1,s=1,v=1,X=1", &[10, 20, 30, 255]));
    assert_eq!(
        frames_of(&rig, 7)[1],
        [0, 0, 0, 0, 10, 20, 30, 255],
        "what the data does not cover is blank"
    );
}

#[test]
fn a_frame_number_past_the_last_makes_a_new_frame() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=7", &[1, 2, 3, 255]));
    let answers = answered(&mut rig, "a=f,i=7,r=9,s=1,v=1,X=1", &[5, 5, 5, 255]);
    assert_eq!(answers, error_of(&frame_ok(7, 9)));
    assert_eq!(frames_of(&rig, 7), [vec![1, 2, 3, 255], vec![5, 5, 5, 255]]);
}

#[test]
fn editing_a_frame_changes_its_gap_only_when_one_is_given() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,i=7", &[1, 2, 3, 255]));
    rig.run(&wire("a=f,i=7,s=1,v=1", &[5, 5, 5, 255]));
    rig.run(&wire("a=f,i=7,r=2,s=1,v=1,X=1", &[6, 6, 6, 255]));
    assert_eq!(gaps_of(&rig, 7), [0, 40]);
    rig.run(&wire("a=f,i=7,r=2,s=1,v=1,X=1,z=70", &[6, 6, 6, 255]));
    assert_eq!(gaps_of(&rig, 7), [0, 70]);
}

// ─── what a frame refuses ───────────────────────────────────────────────

#[test]
fn a_frame_that_cannot_be_made_is_refused_with_the_images_frames_as_they_were() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &[1, 2, 3, 255, 4, 5, 6, 255]));
    let before = frames_of(&rig, 7);
    // Out of the image on the right, bigger than it, and below it.
    for (control, bytes) in [
        ("a=f,i=7,r=1,x=1,s=2,v=1", 8),
        ("a=f,i=7,s=3,v=1", 12),
        ("a=f,i=7,y=1,s=1,v=1", 4),
    ] {
        let answers = answered(&mut rig, control, &vec![0; bytes]);
        assert!(answers.contains("EINVAL:"), "{control}: {answers}");
    }
    let answers = answered(&mut rig, "a=f,i=9,s=1,v=1", &[0; 4]);
    assert!(answers.starts_with("\x1b_Gi=9;ENOENT:"), "{answers}");
    let answers = answered(&mut rig, "a=f,i=7,c=4,s=1,v=1", &[0; 4]);
    assert!(
        answers.starts_with("\x1b_Gi=7;ENOENT:"),
        "no frame 4 to start from"
    );
    let answers = answered(&mut rig, "a=f,i=7,r=1,s=1,v=1", &[0; 3]);
    assert!(answers.contains("ENODATA"), "{answers}");
    assert_eq!(frames_of(&rig, 7), before);
}

#[test]
fn a_frame_names_its_image_by_number_too() {
    let mut rig = Rig::new();
    rig.run(&wire("a=t,f=32,s=1,v=1,I=5", &[1, 2, 3, 255]));
    let (_, answers) = rig.run(&wire("a=f,I=5,r=1,X=1", &[9, 9, 9, 255]));
    assert!(error_of(&answers).ends_with(";OK\x1b\\"), "{answers:?}");
    let ids = rig.main_ids();
    assert_eq!(rig.pixels(ids[0]), Some(vec![9, 9, 9, 255]));
}

#[test]
fn a_frame_may_be_asked_for_before_its_image_is_known_and_the_cursor_stays() {
    let mut rig = Rig::new();
    rig.cursor = (3, 5);
    let (motions, _) = rig.run(&wire("a=f,i=1,s=1,v=1", &[0; 4]));
    assert_eq!(motions, [STAYS]);
    assert!(rig.main_ids().is_empty(), "a frame makes no image");
}

// ─── what the drawing sees ──────────────────────────────────────────────

#[test]
fn an_edit_of_the_frame_on_show_is_a_new_picture_of_the_same_placement() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let before = pictures_of(&rig);
    let placed_before = placed(&rig);
    rig.run(&wire("a=f,i=7,r=1,x=1,s=1,v=1,X=1", &[1, 1, 1, 1]));
    let after = pictures_of(&rig);
    assert_ne!(before.grid()[0].image, after.grid()[0].image, "a new name");
    assert_eq!(placed(&rig), placed_before);
    assert_eq!(after.grid()[0].pixels[4..8], [1, 1, 1, 1]);
    assert_eq!(
        before.grid()[0].pixels[4..8],
        rgba(4, 3, 1)[4..8],
        "what was handed out is not changed under whoever draws it"
    );
}

#[test]
fn a_frame_that_is_not_on_show_does_not_rename_the_picture() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    rig.run(&wire("a=f,i=7,s=1,v=1", &[1, 1, 1, 1]));
    let revision = revision_of(&rig, 7);
    rig.run(&wire("a=f,i=7,r=2,x=1,s=1,v=1,X=1", &[2, 2, 2, 2]));
    assert_eq!(revision_of(&rig, 7), revision);
    assert_eq!(pictures_of(&rig).grid()[0].pixels[..4], rgba(4, 3, 1)[..4]);
}

#[test]
fn an_image_sent_again_has_a_name_no_edit_has_had() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let mut seen = vec![revision_of(&rig, 7)];
    for _ in 0..3 {
        rig.run(&wire("a=f,i=7,r=1,s=1,v=1,X=1", &[1, 1, 1, 1]));
        seen.push(revision_of(&rig, 7));
    }
    shown_image(&mut rig, &rgba(4, 3, 9));
    seen.push(revision_of(&rig, 7));
    let mut once = seen.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), seen.len(), "{seen:?}");
}

#[test]
fn an_edit_tells_the_drawing_which_rectangle_changed() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let before = pictures_of(&rig);
    rig.run(&wire("a=f,i=7,r=1,x=1,y=1,s=2,v=1,X=1", &[1; 8]));
    let once = pictures_of(&rig);
    assert_eq!(
        once.grid()[0].edits.since(before.grid()[0].image),
        Dirty::Rect([1, 1, 2, 1])
    );
    rig.run(&wire("a=f,i=7,r=1,x=0,y=2,s=1,v=1,X=1", &[2; 4]));
    let twice = pictures_of(&rig);
    assert_eq!(
        twice.grid()[0].edits.since(before.grid()[0].image),
        Dirty::Rect([0, 1, 3, 2]),
        "what covers both"
    );
    assert_eq!(
        twice.grid()[0].edits.since(once.grid()[0].image),
        Dirty::Rect([0, 2, 1, 1]),
        "what a copy that has the first edit is missing"
    );
    assert_eq!(
        twice.grid()[0].edits.since(twice.grid()[0].image),
        Dirty::Nothing
    );
}

#[test]
fn a_composed_rectangle_is_an_edit_of_the_frame_on_show() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    rig.run(&wire("a=f,i=7,s=1,v=1", &[3, 3, 3, 255]));
    let before = pictures_of(&rig);
    rig.run(&wire("a=c,i=7,r=2,c=1,x=3,y=2,w=1,h=1,X=0,C=1", &[]));
    let after = pictures_of(&rig);
    assert_eq!(
        after.grid()[0].edits.since(before.grid()[0].image),
        Dirty::Rect([3, 2, 1, 1])
    );
}

#[test]
fn the_name_of_the_image_holds_through_edits_and_not_through_a_resend() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let first = pictures_of(&rig);
    rig.run(&wire("a=f,i=7,r=1,s=1,v=1,X=1", &[1, 1, 1, 1]));
    let edited = pictures_of(&rig);
    assert_eq!(first.grid()[0].key, edited.grid()[0].key);
    shown_image(&mut rig, &rgba(4, 3, 9));
    let sent = pictures_of(&rig);
    assert_ne!(first.grid()[0].key, sent.grid()[0].key);
    assert_eq!(
        sent.grid()[0].edits.since(edited.grid()[0].image),
        Dirty::Whole,
        "a copy of the image that was replaced is not brought up to date"
    );
}

#[test]
fn another_frame_shown_changes_all_of_the_image() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    rig.run(&wire("a=f,i=7,s=1,v=1", &[3, 3, 3, 255]));
    let before = pictures_of(&rig);
    // The frame on show goes, and the one after it is on show.
    rig.run(&wire("a=d,d=f,i=7,r=1", &[]));
    let after = pictures_of(&rig);
    assert_eq!(
        after.grid()[0].edits.since(before.grid()[0].image),
        Dirty::Rect([0, 0, 4, 3])
    );
}

#[test]
fn a_copy_that_fell_further_behind_than_the_edits_remembered_takes_all() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let start = pictures_of(&rig).grid()[0].image;
    let mut seen = Vec::new();
    for step in 0..9u8 {
        rig.run(&wire("a=f,i=7,r=1,s=1,v=1,X=1", &[step, 0, 0, 255]));
        seen.push(pictures_of(&rig).grid()[0].image);
    }
    let last = pictures_of(&rig);
    let edits = &last.grid()[0].edits;
    assert_eq!(edits.since(start), Dirty::Whole);
    assert_eq!(edits.since(seen[0]), Dirty::Rect([0, 0, 1, 1]));
    assert_eq!(edits.since(seen[8]), Dirty::Nothing);
}

/// The pixels of the frame on show as the store holds them.
fn stored_pixels(rig: &Rig, id: u32) -> Option<Arc<Vec<u8>>> {
    rig.hub.with_images(|images| {
        images
            .main()
            .get(id)
            .and_then(|image| image.shown())
            .map(|frame| Arc::clone(&frame.pixels))
    })
}

fn address_of(rig: &Rig, id: u32) -> Option<*const Vec<u8>> {
    stored_pixels(rig, id).map(|pixels| Arc::as_ptr(&pixels))
}

#[test]
fn what_a_frame_released_is_edited_in_place_and_what_it_holds_is_copied() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    let mut pictures = pictures_of(&rig);
    let held = stored_pixels(&rig, 7).expect("stored");
    assert_eq!(
        Arc::strong_count(&held),
        3,
        "the store, the picture and this test's"
    );
    drop(held);
    pictures.release_pixels(|_| true);
    let held = stored_pixels(&rig, 7).expect("stored");
    assert_eq!(Arc::strong_count(&held), 2, "the store's and this test's");
    drop(held);
    assert_eq!(pictures.grid().len(), 1, "where it goes is kept");
    let before = address_of(&rig, 7);
    rig.run(&wire("a=f,i=7,r=1,s=1,v=1,X=1", &[1, 1, 1, 1]));
    assert_eq!(
        address_of(&rig, 7),
        before,
        "released pixels are changed where they lie"
    );
    // Drawn from, they are copied rather than changed under the drawing.
    let drawing = pictures_of(&rig);
    let before = address_of(&rig, 7);
    rig.run(&wire("a=f,i=7,r=1,s=1,v=1,X=1", &[2, 2, 2, 2]));
    assert_ne!(address_of(&rig, 7), before);
    assert_eq!(drawing.grid()[0].pixels[..4], [1, 1, 1, 1]);
}

// ─── composing frames ───────────────────────────────────────────────────

/// Two frames of an image 2 × 1 sent as `i=7`: [1,2,3,255 | 4,5,6,255] and a
/// second one of the same.
fn two_frames(rig: &mut Rig) {
    rig.run(&wire("a=t,f=32,s=2,v=1,i=7", &[1, 2, 3, 255, 4, 5, 6, 255]));
    rig.run(&wire("a=f,i=7,c=1,s=1,v=1,X=1", &[9, 9, 9, 255]));
}

#[test]
fn a_rectangle_of_one_frame_is_laid_on_another() {
    let mut rig = Rig::new();
    two_frames(&mut rig);
    // The root's second pixel goes onto the second frame's first.
    let (motions, answers) = rig.run(&wire("a=c,i=7,r=1,c=2,w=1,h=1,X=1,x=0,C=1", &[]));
    assert_eq!(motions, [STAYS]);
    assert_eq!(answers, ok(7));
    assert_eq!(
        frames_of(&rig, 7),
        [
            vec![1, 2, 3, 255, 4, 5, 6, 255],
            vec![4, 5, 6, 255, 4, 5, 6, 255]
        ]
    );
}

#[test]
fn a_rectangle_may_be_laid_on_another_of_its_own_frame_when_they_do_not_meet() {
    let mut rig = Rig::new();
    two_frames(&mut rig);
    let answers = answered(&mut rig, "a=c,i=7,r=1,c=1,w=1,h=1,X=1,x=0,C=1", &[]);
    assert_eq!(answers, error_of(&ok(7)));
    assert_eq!(frames_of(&rig, 7)[0], [4, 5, 6, 255, 4, 5, 6, 255]);
}

#[test]
fn a_composition_blends_unless_it_is_told_to_replace() {
    let mut rig = Rig::new();
    rig.run(&wire(
        "a=t,f=32,s=2,v=1,i=7",
        &[100, 0, 0, 255, 0, 200, 0, 128],
    ));
    rig.run(&wire("a=f,i=7,c=1,X=1", &[100, 0, 0, 255, 0, 200, 0, 128]));
    rig.run(&wire("a=c,i=7,r=1,c=2,w=1,h=1,X=1", &[]));
    assert_eq!(
        frames_of(&rig, 7)[1][..4],
        [50, 100, 0, 255],
        "half green over red"
    );
}

#[test]
fn a_composition_that_cannot_be_done_is_refused_with_the_frames_as_they_were() {
    let mut rig = Rig::new();
    two_frames(&mut rig);
    let before = frames_of(&rig, 7);
    for (control, code) in [
        ("a=c,i=7,r=1,c=2,x=2,w=1,h=1", "EINVAL"),
        ("a=c,i=7,r=1,c=2,X=2,w=1,h=1", "EINVAL"),
        ("a=c,i=7,r=1,c=2,w=3,h=1", "EINVAL"),
        ("a=c,i=7,r=1,c=2,x=1", "EINVAL"),
        ("a=c,i=7,r=1,c=1,w=2,h=1,X=0,x=1", "EINVAL"),
        ("a=c,i=7,r=1,c=1,w=1,h=1", "EINVAL"),
        ("a=c,i=7,r=3,c=2", "ENOENT"),
        ("a=c,i=7,r=1,c=3", "ENOENT"),
        ("a=c,i=7,r=0,c=1", "ENOENT"),
        ("a=c,i=9,r=1,c=2", "ENOENT"),
    ] {
        let answers = answered(&mut rig, control, &[]);
        assert!(answers.contains(code), "{control}: {answers}");
        assert_eq!(frames_of(&rig, 7), before, "{control}");
    }
}

#[test]
fn composing_on_the_frame_on_show_renames_the_picture() {
    let mut rig = Rig::new();
    shown_image(&mut rig, &rgba(4, 3, 1));
    rig.run(&wire("a=f,i=7,s=1,v=1", &[1, 1, 1, 1]));
    let before = revision_of(&rig, 7);
    rig.run(&wire("a=c,i=7,r=1,c=2,w=1,h=1", &[]));
    assert_eq!(revision_of(&rig, 7), before, "frame 2 is not the one shown");
    rig.run(&wire("a=c,i=7,r=2,c=1,w=1,h=1,C=1", &[]));
    assert_ne!(revision_of(&rig, 7), before);
}

// ─── deleting frames ────────────────────────────────────────────────────

fn three_frames(rig: &mut Rig) {
    rig.run(&wire("a=t,f=32,s=1,v=1,i=7", &[1, 1, 1, 255]));
    rig.run(&wire("a=f,i=7,s=1,v=1,X=1", &[2, 2, 2, 255]));
    rig.run(&wire("a=f,i=7,s=1,v=1,X=1", &[3, 3, 3, 255]));
}

#[test]
fn a_delete_of_a_frame_takes_the_one_named_and_what_it_held_goes_from_the_quota() {
    let mut rig = Rig::new();
    three_frames(&mut rig);
    assert_eq!(rig.hub.with_images(|images| images.main().bytes()), 12);
    rig.run(&wire("a=d,d=f,i=7,r=2", &[]));
    assert_eq!(frames_of(&rig, 7), [vec![1, 1, 1, 255], vec![3, 3, 3, 255]]);
    assert_eq!(rig.hub.with_images(|images| images.main().bytes()), 8);
    rig.run(&wire("a=d,d=F,i=7,r=9", &[]));
    assert_eq!(
        frames_of(&rig, 7),
        [vec![1, 1, 1, 255]],
        "past the last is the last"
    );
}

#[test]
fn deleting_the_first_frame_makes_the_next_the_image_and_renames_the_picture() {
    let mut rig = Rig::new();
    three_frames(&mut rig);
    rig.run(&wire("a=p,i=7,C=1", &[]));
    let before = revision_of(&rig, 7);
    rig.run(&wire("a=d,d=f,i=7", &[]));
    assert_eq!(rig.pixels(7), Some(vec![2, 2, 2, 255]));
    assert_ne!(revision_of(&rig, 7), before);
    assert_eq!(placed(&rig).len(), 1, "the placement stays");
}

#[test]
fn deleting_the_frames_of_an_image_with_one_frame_takes_the_image_only_in_upper_case() {
    let mut rig = Rig::new();
    rig.run(&wire("a=T,f=32,s=1,v=1,i=7,C=1", &[1, 1, 1, 255]));
    rig.run(&wire("a=d,d=f,i=7", &[]));
    assert_eq!(rig.main_ids(), [7]);
    rig.run(&wire("a=d,d=F,i=7", &[]));
    assert!(rig.main_ids().is_empty());
    assert!(placed(&rig).is_empty(), "its placements go with it");
}

#[test]
fn a_delete_of_frames_with_no_image_to_name_does_nothing() {
    let mut rig = Rig::new();
    three_frames(&mut rig);
    rig.run(&wire("a=d,d=f", &[]));
    rig.run(&wire("a=d,d=f,i=99", &[]));
    assert_eq!(frames_of(&rig, 7).len(), 3);
}

// ─── the quota ──────────────────────────────────────────────────────────

fn frame_command(extra: &str) -> Command {
    parsed(&format!("a=f,i=1,s=1,v=5,{extra}"))
}

#[test]
fn frames_count_against_the_quota_and_take_other_images_out_to_fit() {
    let mut images = Images::with_quota(100);
    let screen = Screen::blank(false);
    for id in 1..=3 {
        images.store(
            &screen,
            &parsed(&format!("i={id}")),
            decoded(20),
            Motion::NONE,
        );
    }
    // 1 is the image being given frames; 2 and 3 are not shown.
    let reply = images.frame(&screen, &frame_command(""), decoded(20));
    assert_eq!(reply.encode(), Some(ok(1)));
    assert_eq!(images.main().bytes(), 80);
    images.frame(&screen, &frame_command(""), decoded(20));
    images.frame(&screen, &frame_command(""), decoded(20));
    // 1 holds 80 bytes and 3 holds 20: 2 was the oldest unshown and went.
    assert_eq!(images.main().ids(), [1, 3]);
    assert!(images.main().bytes() <= 100);
}

#[test]
fn a_frame_that_cannot_fit_even_alone_is_refused_and_the_image_keeps_the_ones_it_has() {
    let mut images = Images::with_quota(100);
    let screen = Screen::blank(false);
    images.store(&screen, &parsed("i=1"), decoded(60), Motion::NONE);
    images.store(&screen, &parsed("i=2"), decoded(20), Motion::NONE);
    let big = Decoded {
        width: 1,
        height: 15,
        pixels: vec![0; 60],
    };
    let reply = images.frame(&screen, &parsed("a=f,i=1,s=1,v=15"), big);
    assert!(
        String::from_utf8_lossy(&reply.encode().expect("answered")).contains("ENOSPC"),
        "60 + 60 is more than 100 and the image is not the one to go"
    );
    assert_eq!(
        images.main().ids(),
        [1, 2],
        "nothing goes for a frame that cannot fit"
    );
    assert_eq!(images.main().bytes(), 80);
}

#[test]
fn an_image_keeps_no_more_frames_than_the_limit() {
    let mut images = Images::with_quota(QUOTA_BYTES);
    let screen = Screen::blank(false);
    images.store(
        &screen,
        &parsed("i=1"),
        Decoded {
            width: 1,
            height: 1,
            pixels: vec![0; 4],
        },
        Motion::NONE,
    );
    let one = || Decoded {
        width: 1,
        height: 1,
        pixels: vec![0; 4],
    };
    let command = parsed("a=f,i=1,s=1,v=1");
    for _ in 1..IMAGE_LIMIT {
        assert_eq!(images.frame(&screen, &command, one()).encode(), Some(ok(1)));
    }
    let reply = images.frame(&screen, &command, one());
    assert!(String::from_utf8_lossy(&reply.encode().expect("answered")).contains("ENOSPC"));
    assert_eq!(
        images.main().get(1).map(|image| image.frames.len()),
        Some(IMAGE_LIMIT)
    );
}

// ─── the feature query ──────────────────────────────────────────────────

/// What a feature query comes to: the answer the program reads.
pub(super) fn features(rig: &mut Rig, control: &str) -> String {
    rig.queries.clear();
    let (motions, answers) = rig.run(&wire(control, &[]));
    assert!(motions.is_empty() && answers.is_empty(), "{control}");
    error_of(&rig.queries.concat())
}

#[test]
fn a_feature_query_answers_with_the_keys_that_work() {
    let mut rig = Rig::new();
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=f,r=1,X=1"),
        "\x1b_Gi=31,e=f,r=1,X=1;OK\x1b\\"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=T,C=1,f=100,t=s,o=z,z=-3"),
        "\x1b_Gi=31,e=T,C=1,f=100,t=s,o=z,z=-3;OK\x1b\\"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=c,C=1,w=3"),
        "\x1b_Gi=31,e=c,C=1,w=3;OK\x1b\\"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=d,d=F,x=2"),
        "\x1b_Gi=31,e=d,d=F,x=2;OK\x1b\\"
    );
}

#[test]
fn a_key_with_its_default_or_one_that_is_not_understood_is_left_out() {
    let mut rig = Rig::new();
    // `X=0`, `r=0` and `f=32` are the defaults; `f=8` is not done yet.
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=f,X=0,r=0,f=32,s=2,f=8"),
        "\x1b_Gi=31,e=f,s=2;OK\x1b\\"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=T,U=1,C=1,P=3,H=2"),
        "\x1b_Gi=31,e=T,U=1,C=1;OK\x1b\\",
        "the keys of a placement this terminal does not do are not claimed"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=T,U=0"),
        "\x1b_Gi=31,e=T;OK\x1b\\"
    );
}

#[test]
fn an_action_that_is_not_done_is_answered_without_e() {
    let mut rig = Rig::new();
    assert_eq!(features(&mut rig, "a=q,i=31,e=Z"), "\x1b_Gi=31;OK\x1b\\");
    assert_eq!(features(&mut rig, "a=q,i=31,e=ff"), "\x1b_Gi=31;OK\x1b\\");
}

#[test]
fn a_feature_query_needs_an_id_and_may_be_made_quiet() {
    let mut rig = Rig::new();
    assert_eq!(features(&mut rig, "a=q,e=f"), "", "nobody to answer");
    assert!(features(&mut rig, "a=q,I=4,e=f").contains("EINVAL"));
    assert_eq!(features(&mut rig, "a=q,i=31,e=f,q=1"), "");
}

#[test]
fn a_feature_query_changes_nothing_and_leaves_a_transmission_in_progress_alone() {
    let mut rig = Rig::new();
    let first = [ESC_G, b"a=t,f=32,s=2,v=1,i=5,m=1;AQID", ST].concat();
    rig.run(&first);
    features(&mut rig, "a=q,i=31,e=f,r=1");
    features(&mut rig, "a=q,i=5,e=T");
    let (motions, answers) = rig.run(&[ESC_G, b"m=0;BAUGBwg=", ST].concat());
    assert_eq!(motions, [STAYS]);
    assert_eq!(answers, ok(5));
    assert_eq!(rig.pixels(5), Some(vec![1, 2, 3, 4, 5, 6, 7, 8]));
    assert!(rig.main_ids() == [5], "the query stored nothing");
}

#[test]
fn the_e_key_of_any_other_action_is_refused() {
    let mut rig = Rig::new();
    let answers = answered(&mut rig, "a=t,i=2,e=f,s=1,v=1", &[0; 4]);
    assert!(answers.starts_with("\x1b_Gi=2;EINVAL:"), "{answers}");
    assert!(rig.main_ids().is_empty());
}
