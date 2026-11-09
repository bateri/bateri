//! Unicode placeholders: the virtual placement a program makes, what ends it,
//! and how the cells that hold the placeholder character become pictures.

use super::*;

/// Every virtual placement of the main screen, as (image id, placement id,
/// columns, rows).
fn virtuals(rig: &Rig) -> Vec<(u32, u32, u32, u32)> {
    rig.hub.with_images(|images| images.main().virtuals())
}

fn send(rig: &mut Rig, control: &str) -> Vec<u8> {
    rig.run(&wire(control, &[])).1
}

/// An image of one pixel, sent as `i=id`.
fn one_pixel(rig: &mut Rig, id: u32) {
    rig.run(&wire(&format!("a=t,f=32,s=1,v=1,i={id}"), &[0; 4]));
}

#[test]
fn a_virtual_placement_covers_no_cell_and_leaves_the_cursor_where_it_is() {
    let mut rig = Rig::new();
    rig.cursor = (3, 5);
    let (motions, answers) = rig.run(&wire(
        "a=T,f=32,s=20,v=40,i=1,U=1,c=4,r=2",
        &rgba(20, 40, 0),
    ));
    assert_eq!(motions, [STAYS]);
    assert_eq!(answers, ok(1));
    assert!(placed(&rig).is_empty(), "it is on no line of the grid");
    assert_eq!(virtuals(&rig), [(1, 0, 4, 2)]);
    // The same for a put of an image that is already stored.
    let (motions, answers) = rig.run(&wire("a=p,i=1,U=1,p=3,c=2,r=1", &[]));
    assert_eq!(motions, [STAYS]);
    assert!(error_of(&answers).ends_with("OK\x1b\\"), "{answers:?}");
    assert_eq!(virtuals(&rig), [(1, 0, 4, 2), (1, 3, 2, 1)]);
    assert!(placed(&rig).is_empty());
}

#[test]
fn a_virtual_placement_of_an_image_that_is_not_there_is_an_error() {
    let mut rig = Rig::new();
    let answers = send(&mut rig, "a=p,i=9,U=1,c=1,r=1");
    assert!(error_of(&answers).contains("ENOENT"), "{answers:?}");
    assert!(virtuals(&rig).is_empty());
}

#[test]
fn a_placement_id_moves_the_placement_it_names_whichever_kind_it_is() {
    let mut rig = Rig::new();
    one_pixel(&mut rig, 1);
    send(&mut rig, "a=p,i=1,U=1,p=3,c=2,r=1");
    send(&mut rig, "a=p,i=1,U=1,p=3,c=5,r=4");
    assert_eq!(virtuals(&rig), [(1, 3, 5, 4)], "the id was the same one");
    // One with no id is always another.
    send(&mut rig, "a=p,i=1,U=1,c=1,r=1");
    send(&mut rig, "a=p,i=1,U=1,c=1,r=1");
    assert_eq!(virtuals(&rig).len(), 3);
    // A real placement with the id takes the virtual one's place, and the
    // other way round.
    send(&mut rig, "a=p,i=1,p=3,C=1");
    assert_eq!(virtuals(&rig).len(), 2);
    assert_eq!(placed(&rig).len(), 1);
    send(&mut rig, "a=p,i=1,U=1,p=3,c=1,r=1");
    assert!(placed(&rig).is_empty());
    assert!(virtuals(&rig).contains(&(1, 3, 1, 1)));
}

#[test]
fn only_the_deletes_that_name_the_image_reach_a_virtual_placement() {
    let mut rig = Rig::new();
    one_pixel(&mut rig, 1);
    send(&mut rig, "a=p,i=1,U=1,p=1,c=1,r=1");
    send(&mut rig, "a=p,i=1,U=1,p=2,c=1,r=1");
    for control in [
        "a=d,d=a",
        "a=d,d=A",
        "a=d,d=c",
        "a=d,d=p,x=1,y=1",
        "a=d,d=q,x=1,y=1,z=0",
        "a=d,d=x,x=1",
        "a=d,d=y,y=1",
        "a=d,d=z,z=0",
        "a=d,d=Z,z=0",
        "a=d,d=f,i=1",
    ] {
        send(&mut rig, control);
        assert_eq!(virtuals(&rig).len(), 2, "{control}");
    }
    assert_eq!(rig.main_ids(), [1], "the image stays under a virtual one");
    send(&mut rig, "a=d,d=i,i=1,p=2");
    assert_eq!(virtuals(&rig), [(1, 1, 1, 1)]);
    send(&mut rig, "a=d,d=i,i=1");
    assert!(virtuals(&rig).is_empty());
    assert_eq!(
        rig.main_ids(),
        [1],
        "d=i takes the placements, not the data"
    );
}

#[test]
fn the_data_goes_with_the_last_virtual_placement_in_upper_case() {
    let mut rig = Rig::new();
    one_pixel(&mut rig, 1);
    send(&mut rig, "a=p,i=1,U=1,p=1,c=1,r=1");
    send(&mut rig, "a=p,i=1,U=1,p=2,c=1,r=1");
    send(&mut rig, "a=d,d=I,i=1,p=1");
    assert_eq!(rig.main_ids(), [1], "a placement is left");
    send(&mut rig, "a=d,d=I,i=1,p=2");
    assert!(rig.main_ids().is_empty());
}

#[test]
fn deleting_by_number_or_a_range_of_ids_reaches_a_virtual_placement() {
    let mut rig = Rig::new();
    for id in [1, 2, 3] {
        one_pixel(&mut rig, id);
        send(&mut rig, &format!("a=p,i={id},U=1,c=1,r=1"));
    }
    send(&mut rig, "a=d,d=r,x=2,y=3");
    assert_eq!(virtuals(&rig), [(1, 0, 1, 1)]);
    rig.run(&wire("a=t,f=32,s=1,v=1,I=7", &[0; 4]));
    send(&mut rig, "a=p,I=7,U=1,c=2,r=2");
    assert_eq!(virtuals(&rig).len(), 2);
    send(&mut rig, "a=d,d=n,I=7");
    assert_eq!(virtuals(&rig), [(1, 0, 1, 1)]);
}

#[test]
fn sending_an_id_again_ends_its_virtual_placements_with_the_image() {
    let mut rig = Rig::new();
    one_pixel(&mut rig, 1);
    send(&mut rig, "a=p,i=1,U=1,c=3,r=3");
    one_pixel(&mut rig, 1);
    assert!(virtuals(&rig).is_empty());
}

#[test]
fn clearing_the_screen_leaves_a_virtual_placement_and_a_reset_takes_it() {
    let mut rig = Rig::new();
    one_pixel(&mut rig, 1);
    send(&mut rig, "a=p,i=1,U=1,c=3,r=3");
    rig.hub.with_images_mut(|images| {
        images.clear_screen(&Screen::blank(false), true);
        images.wiped(0, (10, 20));
        images.resized(3, false, (10, 20));
    });
    assert_eq!(virtuals(&rig).len(), 1, "it has no line to be cleared with");
    rig.hub.with_images_mut(|images| images.reset());
    assert!(virtuals(&rig).is_empty());
}

#[test]
fn an_image_that_only_a_virtual_placement_shows_is_the_last_the_quota_takes() {
    let mut images = Images::with_quota(100);
    for id in 1..=3 {
        images.store(
            &Screen::blank(false),
            &parsed(&format!("i={id}")),
            decoded(30),
            Motion::NONE,
        );
    }
    images.put(
        &Screen::blank(false),
        &parsed("a=p,i=1,U=1,c=1,r=1"),
        Motion::NONE,
    );
    images.store(
        &Screen::blank(false),
        &parsed("i=4"),
        decoded(30),
        Motion::NONE,
    );
    // 120 > 100: the oldest that nothing shows (2) goes, not 1, which a
    // placement made the newest.
    assert_eq!(images.main().ids(), [3, 1, 4]);
}
