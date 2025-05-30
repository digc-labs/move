// Copyright (c) The Diem Core Contributors
// Copyright (c) The Move Contributors
// Modifications Copyright (c) 2024 Digc Labs
// SPDX-License-Identifier: Apache-2.0

use crate::{account_address::AccountAddress, identifier::Identifier, language_storage::ModuleId};
use anyhow::{bail, Result};
use std::collections::btree_map::{self, BTreeMap};

/// A storage operation.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum Op<T> {
    /// Inserts some new data into an empty slot.
    New(T),
    /// Modifies some data that currently exists.
    Modify(T),
    /// Deletes some data that currently exists.
    Delete,
}

impl<T> Op<T> {
    pub fn as_ref(&self) -> Op<&T> {
        use Op::*;

        match self {
            New(data) => New(data),
            Modify(data) => Modify(data),
            Delete => Delete,
        }
    }

    pub fn map<F, U>(self, f: F) -> Op<U>
    where
        F: FnOnce(T) -> U,
    {
        use Op::*;

        match self {
            New(data) => New(f(data)),
            Modify(data) => Modify(f(data)),
            Delete => Delete,
        }
    }

    pub fn ok(self) -> Option<T> {
        use Op::*;

        match self {
            New(data) | Modify(data) => Some(data),
            Delete => None,
        }
    }
}

/// A collection of resource and module operations on a Move account.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AccountChangeSet {
    modules: BTreeMap<Identifier, Op<Vec<u8>>>,
}

/// This implements an algorithm to squash two change sets together by merging pairs of operations
/// on the same item together. This is similar to squashing two commits in a version control system.
///
/// It should be noted that all operation types have some implied pre and post conditions:
///   - New
///     - before: data doesn't exist
///     - after: data exists (new)
///   - Modify
///     - before: data exists
///     - after: data exists (modified)
///   - Delete
///     - before: data exists
///     - after: data does not exist (deleted)
///
/// It is possible to have a pair of operations resulting in conflicting states, in which case the
/// squash will fail.
fn squash<K, V>(map: &mut BTreeMap<K, Op<V>>, other: BTreeMap<K, Op<V>>) -> Result<()>
where
    K: Ord,
{
    use btree_map::Entry::*;
    use Op::*;

    for (key, op) in other.into_iter() {
        match map.entry(key) {
            Occupied(mut entry) => {
                let r = entry.get_mut();
                match (r.as_ref(), op) {
                    (Modify(_) | New(_), New(_)) | (Delete, Delete | Modify(_)) => {
                        bail!("The given change sets cannot be squashed")
                    }
                    (Modify(_), Modify(data)) => *r = Modify(data),
                    (New(_), Modify(data)) => *r = New(data),
                    (Modify(_), Delete) => *r = Delete,
                    (Delete, New(data)) => *r = Modify(data),
                    (New(_), Delete) => {
                        entry.remove();
                    }
                }
            }
            Vacant(entry) => {
                entry.insert(op);
            }
        }
    }

    Ok(())
}

impl AccountChangeSet {
    pub fn from_modules(modules: BTreeMap<Identifier, Op<Vec<u8>>>) -> Self {
        Self { modules }
    }

    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            modules: BTreeMap::new(),
        }
    }

    pub fn add_module_op(&mut self, name: Identifier, op: Op<Vec<u8>>) -> Result<()> {
        use btree_map::Entry::*;

        match self.modules.entry(name) {
            Occupied(entry) => bail!("Module {} already exists", entry.key()),
            Vacant(entry) => {
                entry.insert(op);
            }
        }

        Ok(())
    }

    pub fn into_inner(self) -> BTreeMap<Identifier, Op<Vec<u8>>> {
        self.modules
    }

    pub fn into_modules(self) -> BTreeMap<Identifier, Op<Vec<u8>>> {
        self.modules
    }

    pub fn modules(&self) -> &BTreeMap<Identifier, Op<Vec<u8>>> {
        &self.modules
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    pub fn squash(&mut self, other: Self) -> Result<()> {
        squash(&mut self.modules, other.modules)
    }
}

// TODO: ChangeSet does not have a canonical representation so the derived Ord is not sound.

/// A collection of changes to a Move state. Each AccountChangeSet in the domain of `accounts`
/// is guaranteed to be nonempty
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChangeSet {
    accounts: BTreeMap<AccountAddress, AccountChangeSet>,
}

impl Default for ChangeSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeSet {
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    pub fn add_account_changeset(
        &mut self,
        addr: AccountAddress,
        account_changeset: AccountChangeSet,
    ) -> Result<()> {
        match self.accounts.entry(addr) {
            btree_map::Entry::Occupied(_) => bail!(
                "Failed to add account change set. Account {} already exists.",
                addr
            ),
            btree_map::Entry::Vacant(entry) => {
                entry.insert(account_changeset);
            }
        }

        Ok(())
    }

    pub fn accounts(&self) -> &BTreeMap<AccountAddress, AccountChangeSet> {
        &self.accounts
    }

    pub fn into_inner(self) -> BTreeMap<AccountAddress, AccountChangeSet> {
        self.accounts
    }

    fn get_or_insert_account_changeset(&mut self, addr: AccountAddress) -> &mut AccountChangeSet {
        match self.accounts.entry(addr) {
            btree_map::Entry::Occupied(entry) => entry.into_mut(),
            btree_map::Entry::Vacant(entry) => entry.insert(AccountChangeSet::new()),
        }
    }

    pub fn add_module_op(&mut self, module_id: ModuleId, op: Op<Vec<u8>>) -> Result<()> {
        let account = self.get_or_insert_account_changeset(*module_id.address());
        account.add_module_op(module_id.name().to_owned(), op)
    }

    pub fn squash(&mut self, other: Self) -> Result<()> {
        for (addr, other_account_changeset) in other.accounts {
            match self.accounts.entry(addr) {
                btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().squash(other_account_changeset)?;
                }
                btree_map::Entry::Vacant(entry) => {
                    entry.insert(other_account_changeset);
                }
            }
        }
        Ok(())
    }

    pub fn into_modules(self) -> impl Iterator<Item = (ModuleId, Op<Vec<u8>>)> {
        self.accounts.into_iter().flat_map(|(addr, account)| {
            account
                .modules
                .into_iter()
                .map(move |(module_name, blob_opt)| (ModuleId::new(addr, module_name), blob_opt))
        })
    }

    pub fn modules(&self) -> impl Iterator<Item = (AccountAddress, &Identifier, Op<&[u8]>)> {
        self.accounts.iter().flat_map(|(addr, account)| {
            let addr = *addr;
            account
                .modules
                .iter()
                .map(move |(module_name, op)| (addr, module_name, op.as_ref().map(|v| v.as_ref())))
        })
    }
}
