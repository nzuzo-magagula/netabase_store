//! Subscription semantics on a minimal three-tier schema: gating (a level's own topic is written
//! only when requested), per-topic selectivity, wrapped-PK keying at each level, delete removing
//! rows, fan-out, and merkle-root divergence on differing content.
use netabase_arena::fixed::NbString;
use netabase_macros::{netabase_definition, netabase_model, netabase_repository};
use netabase_store::databases::memory::MemoryStore;
use netabase_store::traits::structural::database::NetabaseStore;
use netabase_store::traits::structural::database::tables::core::ModelHash;
use netabase_store::traits::structural::database::tables::{InsertConfig, InsertPolicy, TableReadOps};
use netabase_store::traits::structural::database::transactions::repository::{
    RepositoryTransaction, RepositoryWriteTx,
};
use netabase_store::traits::structural::schema::models::keys::subscription::SubscriptionRegistry;
use netabase_store::traits::structural::schema::repositories::NetabaseRepository;
use rkyv::{Archive, Deserialize, Serialize};

#[netabase_repository(SubRepo, subscriptions(RepoAll))]
pub mod repo {
    use super::*;

    #[netabase_definition(SubDef, repository(SubRepo), subscriptions(DefAll))]
    pub mod definition {
        use super::*;

        #[netabase_model]
        #[derive(Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(SubDef), subscriptions(OnCreate, OnUpdate))]
        pub struct Note {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub body: NbString<16>,
        }
    }
}

use repo::SubDef;
use repo::definition::{Note, NotePrimaryKey, NoteSubscriptionKeys};
use repo::{SubDefPrimaryKey, SubDefSubscriptionKeys, SubDefSubscriptionRegistry};

fn note(id: u64, body: &str) -> Note {
    Note {
        id: NotePrimaryKey(id),
        body: NbString::try_from_str(body).unwrap(),
    }
}

fn repo_all() -> SubRepoSubscriptionKeys {
    SubRepoSubscriptionKeys::RepoAll
}
fn def_all() -> SubRepoSubscriptionKeys {
    SubRepoSubscriptionKeys::SubDef(SubDefSubscriptionKeys::DefAll)
}
fn note_topic(t: NoteSubscriptionKeys) -> SubRepoSubscriptionKeys {
    SubRepoSubscriptionKeys::SubDef(SubDefSubscriptionKeys::Note(t))
}

fn insert<P: InsertPolicy>(
    store: &mut MemoryStore<SubRepoItem>,
    n: Note,
    cfg: &InsertConfig<'_, SubRepoSubscriptionKeys, P>,
) {
    let mut txn = store.write_transaction().unwrap();
    SubRepoItem::SubDef(SubDef::Note(n))
        .route_insert::<MemoryStore<SubRepoItem>, P>(&mut txn, cfg)
        .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();
}

#[test]
fn gating_writes_only_requested_topics_at_each_level() {
    let mut store = MemoryStore::<SubRepoItem>::open(()).unwrap();

    let topics = [
        repo_all(),
        def_all(),
        note_topic(NoteSubscriptionKeys::OnCreate),
    ];
    insert(&mut store, note(1, "a"), &InsertConfig::new().subscriptions(&topics));
    insert(&mut store, note(2, "b"), &InsertConfig::new());

    let txn = store.read_transaction().unwrap();

    let model_t = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnCreate").unwrap();
    assert!(model_t.get(&NotePrimaryKey(1)).unwrap().is_some(), "requested → written");
    assert!(model_t.get(&NotePrimaryKey(2)).unwrap().is_none(), "not requested → absent");

    let def_t = txn.open_read_table::<SubDef, SubDefPrimaryKey, ModelHash>("SubDef_DefAll").unwrap();
    assert!(def_t.get(&SubDefPrimaryKey::Note(NotePrimaryKey(1))).unwrap().is_some());
    assert!(def_t.get(&SubDefPrimaryKey::Note(NotePrimaryKey(2))).unwrap().is_none());

    let repo_t = txn.open_read_table::<SubRepoItem, SubRepoPrimaryKey, ModelHash>("SubRepo_RepoAll").unwrap();
    let wrap = |id| SubRepoPrimaryKey::SubDef(SubDefPrimaryKey::Note(NotePrimaryKey(id)));
    assert!(repo_t.get(&wrap(1)).unwrap().is_some());
    assert!(repo_t.get(&wrap(2)).unwrap().is_none());
}

#[test]
fn per_topic_selectivity() {
    let mut store = MemoryStore::<SubRepoItem>::open(()).unwrap();

    let t1 = [note_topic(NoteSubscriptionKeys::OnCreate)];
    let t2 = [note_topic(NoteSubscriptionKeys::OnUpdate)];
    insert(&mut store, note(1, "a"), &InsertConfig::new().subscriptions(&t1));
    insert(&mut store, note(2, "b"), &InsertConfig::new().subscriptions(&t2));

    let txn = store.read_transaction().unwrap();
    let on_create = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnCreate").unwrap();
    let on_update = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnUpdate").unwrap();

    assert!(on_create.get(&NotePrimaryKey(1)).unwrap().is_some());
    assert!(on_create.get(&NotePrimaryKey(2)).unwrap().is_none());
    assert!(on_update.get(&NotePrimaryKey(2)).unwrap().is_some());
    assert!(on_update.get(&NotePrimaryKey(1)).unwrap().is_none());
}

#[test]
fn delete_removes_model_subscription_rows() {
    let mut store = MemoryStore::<SubRepoItem>::open(()).unwrap();

    let t = [note_topic(NoteSubscriptionKeys::OnCreate)];
    insert(&mut store, note(7, "x"), &InsertConfig::new().subscriptions(&t));
    {
        let txn = store.read_transaction().unwrap();
        let tbl = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnCreate").unwrap();
        assert!(tbl.get(&NotePrimaryKey(7)).unwrap().is_some(), "present before delete");
    }

    let mut txn = store.write_transaction().unwrap();
    SubRepoItem::route_delete(
        SubRepoAddress::SubDef(repo::SubDefAddress::Note),
        SubRepoPrimaryKey::SubDef(SubDefPrimaryKey::Note(NotePrimaryKey(7))),
        &mut txn,
    )
    .unwrap();
    RepositoryWriteTx::commit(txn).unwrap();

    let txn = store.read_transaction().unwrap();
    let tbl = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnCreate").unwrap();
    assert!(tbl.get(&NotePrimaryKey(7)).unwrap().is_none(), "removed after delete");
}

#[test]
fn fan_out_across_models_and_topics() {
    let mut store = MemoryStore::<SubRepoItem>::open(()).unwrap();

    let d = [def_all()];
    insert(&mut store, note(1, "a"), &InsertConfig::new().subscriptions(&d));
    insert(&mut store, note(2, "b"), &InsertConfig::new().subscriptions(&d));
    let both = [
        note_topic(NoteSubscriptionKeys::OnCreate),
        note_topic(NoteSubscriptionKeys::OnUpdate),
    ];
    insert(&mut store, note(3, "c"), &InsertConfig::new().subscriptions(&both));

    let txn = store.read_transaction().unwrap();

    let def_t = txn.open_read_table::<SubDef, SubDefPrimaryKey, ModelHash>("SubDef_DefAll").unwrap();
    assert!(def_t.get(&SubDefPrimaryKey::Note(NotePrimaryKey(1))).unwrap().is_some());
    assert!(def_t.get(&SubDefPrimaryKey::Note(NotePrimaryKey(2))).unwrap().is_some());
    assert_eq!(def_t.range(..).unwrap().count(), 2, "two distinct definition-topic rows");

    let on_create = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnCreate").unwrap();
    let on_update = txn.open_read_table::<Note, NotePrimaryKey, ModelHash>("Note_OnUpdate").unwrap();
    assert!(on_create.get(&NotePrimaryKey(3)).unwrap().is_some());
    assert!(on_update.get(&NotePrimaryKey(3)).unwrap().is_some());
}

#[test]
fn merkle_root_divergence_on_content() {
    let entry = |id, body| SubDef::Note(note(id, body)).subscription_registry_entry().unwrap();

    let same_a = SubDefSubscriptionRegistry::merkle_root(&[entry(1, "x"), entry(2, "y")]);
    let same_b = SubDefSubscriptionRegistry::merkle_root(&[entry(1, "x"), entry(2, "y")]);
    assert_eq!(same_a, same_b, "identical content → identical root");

    let diverged = SubDefSubscriptionRegistry::merkle_root(&[entry(1, "x"), entry(2, "DIFFERENT")]);
    assert_ne!(same_a, diverged, "differing content → differing root");

    assert_ne!(entry(1, "x").member_hash(), entry(1, "z").member_hash());
}
