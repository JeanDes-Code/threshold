use threshold_core::{
    build::names_pc,
    domain::EntityKind,
    normalize,
    store::{Entity, WorldView},
};

fn entity(id: u64, name: &str, other_names: &[&str]) -> Entity {
    Entity {
        id,
        kind: EntityKind::Npc,
        name: name.into(),
        norm_name: normalize::name(name),
        age: Some(30),
        other_names: other_names.iter().map(|n| (*n).into()).collect(),
    }
}

fn world(entities: Vec<Entity>) -> WorldView {
    WorldView {
        entities,
        ..WorldView::default()
    }
}

fn id_of(view: &WorldView, name: &str) -> Option<u64> {
    view.entity_by_name(name).map(|e| e.id)
}

#[test]
fn resolves_main_and_other_names() {
    let view = world(vec![entity(1, "Orsel", &[]), entity(2, "Jean", &["Jade"])]);
    assert_eq!(id_of(&view, "jade"), Some(2));
    assert_eq!(id_of(&view, "JADE"), Some(2));
    assert_eq!(id_of(&view, "Jean"), Some(2));
    assert_eq!(id_of(&view, "Orsel"), Some(1));
    assert_eq!(id_of(&view, "Hessa"), None);
}

#[test]
fn accents_and_case_share_one_entity() {
    let view = world(vec![entity(1, "Orsel", &[]), entity(4, "Chloé", &[])]);
    assert_eq!(id_of(&view, "Chloé"), Some(4));
    assert_eq!(id_of(&view, "chloe"), Some(4));
    assert_eq!(id_of(&view, "CHLOÉ"), Some(4));

    // The same holds for an other name.
    let view = world(vec![entity(1, "Orsel", &[]), entity(2, "Hessa", &["Léa"])]);
    assert_eq!(id_of(&view, "Lea"), Some(2));
    assert_eq!(id_of(&view, "LÉA"), Some(2));
}

#[test]
fn tie_takes_the_lowest_id() {
    // Out of id order on purpose: the lowest id wins, not the first row, and
    // a main name does not beat an other name.
    let view = world(vec![
        entity(7, "Mara", &[]),
        entity(3, "Hessa", &["mara"]),
        entity(5, "Mara", &[]),
    ]);
    assert_eq!(id_of(&view, "Mara"), Some(3));
}

#[test]
fn normalize_name_ignores_edge_punctuation() {
    assert_eq!(normalize::name("Ostley Crossing."), "ostley crossing");
    assert_eq!(
        normalize::name("Ostley Crossing."),
        normalize::name("ostley crossing")
    );
    assert_eq!(
        normalize::name("  \"The  Copper Kettle,\" "),
        "copper kettle"
    );
    assert_eq!(normalize::name("(Wick)"), "wick");
    // Inner apostrophes and hyphens stay.
    assert_eq!(normalize::name("O'Brien"), "o'brien");
    assert_eq!(normalize::name("O’Brien!"), "o'brien");
    assert_eq!(normalize::name("Jean-Luc"), "jean-luc");
    assert_ne!(normalize::name("Jean-Luc"), normalize::name("Jean Luc"));
    // A name lookup ignores the edge punctuation too.
    let view = world(vec![entity(1, "Ostley Crossing", &["the Ford"])]);
    assert_eq!(id_of(&view, "Ostley Crossing."), Some(1));
    assert_eq!(id_of(&view, "The Ford!"), Some(1));
}

#[test]
fn first_name_alone_names_the_pc() {
    let others = ["Wick".to_owned()];
    let named = |text: &str| names_pc("Teodor Brandt", &others, text);
    assert!(named("Teodor owes her money"));
    assert!(named("She wants Teodor Brandt to sell."));
    assert!(named("Make Wick carry the winter post."));
    assert!(named("Teodor's ferry"));
    assert!(named("(Teodor) pays."));
    // A word is whole: a longer word or a hyphenated word is not the name.
    assert!(!named("Theodora owes her money"));
    assert!(!named("Teodora sells bread."));
    assert!(!named("Save a penny for the Teodor-Brandtson fair."));
    assert!(!named("A wicker basket."));
    // Only the first word of the main name counts alone, not the last.
    assert!(!named("Old Brandt left the village."));
}
