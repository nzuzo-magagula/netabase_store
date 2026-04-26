//! Stacked multi-repository / multi-definition schema with the fixed-width model vocabulary.
//! A model registered to one definition is reachable through every repository that definition
//! belongs to; routing dispatches top-down through the generated enums.
use netabase_arena::fixed::{NbString, NbVec};
use netabase_macros::{NetabaseModel, netabase_definition, netabase_repository};
use netabase_store::databases::redb::RedbStore;
use netabase_store::traits::structural::{
    database::{
        NetabaseStore,
        transactions::repository::{RepositoryWriteOps, RepositoryWriteTx},
    },
    schema::repositories::NetabaseRepository,
};
use rkyv::{Archive, Deserialize, Serialize};

// Access levels as a small integer (fixed-width); 0 = Read, 1 = Write, 2 = Admin.
const ACCESS_ADMIN: u8 = 2;

#[netabase_repository(TireShopRepo)]
#[netabase_repository(ManagementRepo)]
pub mod repo_mod {
    use super::*;

    // Multiple definitions stacked on the same module.
    #[netabase_definition(StaffDef, repository(TireShopRepo, ManagementRepo))]
    #[netabase_definition(SecurityDef, repository(TireShopRepo))]
    pub mod staff_mod {
        use super::*;

        #[derive(NetabaseModel, Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(StaffDef))]
        pub struct Employee {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub name: NbString<32>,
            pub role_ids: NbVec<u64, 8>,
        }

        #[derive(NetabaseModel, Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(StaffDef))]
        pub struct Role {
            #[netabase(PrimaryKey)]
            pub id: u64,
            pub name: NbString<32>,
        }

        #[derive(NetabaseModel, Archive, Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        #[rkyv(derive(Debug))]
        #[netabase(definition(SecurityDef))]
        pub struct Permission {
            #[netabase(PrimaryKey)]
            pub id: NbString<24>,
            pub role_id: u64,
            pub target_table: NbString<32>,
            pub level: u8,
        }
    }
}

fn main() {
    let path = std::env::temp_dir().join(format!("tire_shop_{}.redb", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let mut store = RedbStore::<TireShopRepoItem>::open(path.clone()).unwrap();

    // 1. Seed data through hierarchical routing.
    {
        use repo_mod::staff_mod::{Employee, Permission, Role};
        use repo_mod::{SecurityDef, StaffDef};

        let mut txn = store.write_transaction().unwrap();

        let admin_role = Role {
            id: 1,
            name: NbString::try_from_str("Manager").unwrap(),
        };
        let bob = Employee {
            id: 101,
            name: NbString::try_from_str("Bob").unwrap(),
            role_ids: NbVec::try_from_slice(&[1u64]).unwrap(),
        };
        let perm = Permission {
            id: NbString::try_from_str("1:Employee").unwrap(),
            role_id: 1,
            target_table: NbString::try_from_str("Employee").unwrap(),
            level: ACCESS_ADMIN,
        };

        txn.insert(TireShopRepoItem::StaffDef(StaffDef::Role(admin_role))).unwrap();
        txn.insert(TireShopRepoItem::StaffDef(StaffDef::Employee(bob))).unwrap();
        txn.insert(TireShopRepoItem::SecurityDef(SecurityDef::Permission(perm))).unwrap();

        txn.commit().unwrap();
    }

    // 2. Repo-level dispatch + permission overlap.
    {
        use repo_mod::staff_mod::{EmployeePrimaryKey, PermissionPrimaryKey};
        use repo_mod::{SecurityDef, SecurityDefPrimaryKey, StaffDef, StaffDefPrimaryKey};

        let txn = store.read_transaction().unwrap();

        let res = TireShopRepoItem::route_get(
            &txn,
            TireShopRepoPrimaryKey::StaffDef(StaffDefPrimaryKey::Employee(EmployeePrimaryKey(101))),
        )
        .unwrap();

        let Some(TireShopRepoItem::StaffDef(StaffDef::Employee(bob))) = res else {
            panic!("Bob should be reachable through TireShopRepo");
        };
        println!("Found employee: {} with {} role(s)", bob.name, bob.role_ids.len());

        let mut has_admin = false;
        for role_id in bob.role_ids.iter() {
            let perm_id = format!("{}:Employee", role_id);
            let perm_pk = PermissionPrimaryKey(NbString::try_from_str(&perm_id).unwrap());
            let p_res = TireShopRepoItem::route_get(
                &txn,
                TireShopRepoPrimaryKey::SecurityDef(SecurityDefPrimaryKey::Permission(perm_pk)),
            )
            .unwrap();
            if let Some(TireShopRepoItem::SecurityDef(SecurityDef::Permission(p))) = p_res {
                if p.level == ACCESS_ADMIN {
                    has_admin = true;
                    break;
                }
            }
        }
        println!("Bob has Admin access to Employee via role overlap? {has_admin}");
        assert!(has_admin);
    }

    let _ = std::fs::remove_file(path);
    println!("Tire Shop stacked example finished successfully.");
}
