//! Flat forward storage with shared weight and join-position sequences.

use std::collections::BTreeMap;
use std::mem::size_of;

use super::{AliasForm, AliasInput, AliasJoin, AliasToken, FormId, TokenId};
use crate::{Error, Result};

#[derive(Debug, PartialEq)]
pub(super) struct AliasHeader {
  entry_id: u32,
  entity_group: u32,
  quality: f64,
  forms_start: u32,
  forms_len: u32,
}

#[derive(Debug, PartialEq)]
pub(super) struct FormHeader {
  form: FormId,
  tokens_start: u32,
  tokens_len: u32,
  joins_start: u32,
  joins_len: u32,
  positions_start: u32,
  weights_start: u32,
  minimum_total: f64,
}

#[derive(Debug, PartialEq)]
struct Weight {
  value: f64,
  minimum: f64,
}

#[derive(Debug, PartialEq)]
pub(super) struct ForwardStore {
  aliases: Vec<AliasHeader>,
  forms: Vec<FormHeader>,
  tokens: Vec<TokenId>,
  joins: Vec<TokenId>,
  weights: Vec<Weight>,
  positions: Vec<(u32, u32)>,
}

fn offset(value: usize) -> Result<u32> {
  u32::try_from(value)
    .map_err(|_| Error::from_reason("Entity forward storage exceeds u32"))
}

impl ForwardStore {
  pub(super) fn new(aliases: Vec<AliasInput>) -> Result<Self> {
    let mut store = Self {
      aliases: Vec::with_capacity(aliases.len()),
      forms: Vec::new(),
      tokens: Vec::new(),
      joins: Vec::new(),
      weights: Vec::new(),
      positions: Vec::new(),
    };
    let mut weights = BTreeMap::new();
    let mut positions = BTreeMap::new();
    for alias in aliases {
      if alias.forms.is_empty() {
        return Err(Error::from_reason("Missing entity alias form"));
      }
      let header = AliasHeader {
        entry_id: alias.entry_id,
        entity_group: alias.entity_group,
        quality: alias.quality,
        forms_start: offset(store.forms.len())?,
        forms_len: offset(alias.forms.len())?,
      };
      for form in alias.forms {
        let weights_key = form
          .tokens
          .iter()
          .map(|token| (token.weight.to_bits(), token.minimum_weight.to_bits()))
          .collect::<Vec<_>>();
        let weights_start = if let Some(start) = weights.get(&weights_key) {
          *start
        } else {
          let start = offset(store.weights.len())?;
          store.weights.extend(form.tokens.iter().map(|token| Weight {
            value: token.weight,
            minimum: token.minimum_weight,
          }));
          offset(store.weights.len())?;
          weights.insert(weights_key, start);
          start
        };
        let positions_key = form
          .joins
          .iter()
          .map(|join| (join.left, join.right))
          .collect::<Vec<_>>();
        let positions_start = if let Some(start) = positions.get(&positions_key)
        {
          *start
        } else {
          let start = offset(store.positions.len())?;
          store.positions.extend(&positions_key);
          offset(store.positions.len())?;
          positions.insert(positions_key, start);
          start
        };
        store.forms.push(FormHeader {
          form: form.form,
          tokens_start: offset(store.tokens.len())?,
          tokens_len: offset(form.tokens.len())?,
          joins_start: offset(store.joins.len())?,
          joins_len: offset(form.joins.len())?,
          positions_start,
          weights_start,
          minimum_total: form.minimum_total,
        });
        store
          .tokens
          .extend(form.tokens.iter().map(|token| token.id));
        store.joins.extend(form.joins.iter().map(|join| join.id));
        offset(store.tokens.len())?;
        offset(store.joins.len())?;
      }
      offset(store.forms.len())?;
      store.aliases.push(header);
    }
    store.forms.shrink_to_fit();
    store.tokens.shrink_to_fit();
    store.joins.shrink_to_fit();
    store.weights.shrink_to_fit();
    store.positions.shrink_to_fit();
    Ok(store)
  }

  pub(super) const fn len(&self) -> usize {
    self.aliases.len()
  }

  pub(super) fn get(&self, id: usize) -> Option<AliasView<'_>> {
    self
      .aliases
      .get(id)
      .map(|alias| AliasView::Packed { store: self, alias })
  }

  pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = AliasInput> {
    self
      .aliases
      .iter()
      .map(|alias| AliasView::Packed { store: self, alias }.to_input())
  }

  pub(super) const fn native_bytes(&self) -> usize {
    size_of::<Self>()
      .saturating_add(
        self
          .aliases
          .capacity()
          .saturating_mul(size_of::<AliasHeader>()),
      )
      .saturating_add(
        self
          .forms
          .capacity()
          .saturating_mul(size_of::<FormHeader>()),
      )
      .saturating_add(
        self.tokens.capacity().saturating_mul(size_of::<TokenId>()),
      )
      .saturating_add(
        self.joins.capacity().saturating_mul(size_of::<TokenId>()),
      )
      .saturating_add(
        self.weights.capacity().saturating_mul(size_of::<Weight>()),
      )
      .saturating_add(
        self
          .positions
          .capacity()
          .saturating_mul(size_of::<(u32, u32)>()),
      )
  }
}

/// Search borrows the packed arrays; candidate checks never reconstruct owned aliases.
#[derive(Clone, Copy)]
pub(super) enum AliasView<'a> {
  #[cfg(test)]
  Input(&'a AliasInput),
  Packed {
    store: &'a ForwardStore,
    alias: &'a AliasHeader,
  },
}

#[cfg(test)]
impl<'a> From<&'a AliasInput> for AliasView<'a> {
  fn from(value: &'a AliasInput) -> Self {
    Self::Input(value)
  }
}

// Ranges originate as usize offsets and their ends are checked to fit u32 during
// construction. No serialized offsets enter these immutable arrays.
#[allow(
  clippy::indexing_slicing,
  clippy::as_conversions,
  clippy::cast_possible_truncation,
  clippy::arithmetic_side_effects
)]
impl<'a> AliasView<'a> {
  pub(super) const fn quality(self) -> f64 {
    match self {
      #[cfg(test)]
      Self::Input(alias) => alias.quality,
      Self::Packed { alias, .. } => alias.quality,
    }
  }

  pub(super) fn forms(self) -> impl ExactSizeIterator<Item = FormView<'a>> {
    let len = match self {
      #[cfg(test)]
      Self::Input(alias) => alias.forms.len(),
      Self::Packed { alias, .. } => alias.forms_len as usize,
    };
    (0..len).map(move |position| match self {
      #[cfg(test)]
      Self::Input(alias) => FormView::Input(&alias.forms[position]),
      Self::Packed { store, alias } => FormView::Packed {
        form: &store.forms[alias.forms_start as usize + position],
        store,
      },
    })
  }

  fn to_input(self) -> AliasInput {
    let (entry_id, entity_group) = match self {
      #[cfg(test)]
      Self::Input(alias) => (alias.entry_id, alias.entity_group),
      Self::Packed { alias, .. } => (alias.entry_id, alias.entity_group),
    };
    AliasInput {
      entry_id,
      entity_group,
      quality: self.quality(),
      forms: self
        .forms()
        .map(|form| AliasForm {
          form: form.form(),
          minimum_total: form.minimum_total(),
          tokens: form.tokens().collect(),
          joins: form.joins().collect(),
        })
        .collect(),
    }
  }
}

#[derive(Clone, Copy)]
pub(super) enum FormView<'a> {
  #[cfg(test)]
  Input(&'a AliasForm),
  Packed {
    form: &'a FormHeader,
    store: &'a ForwardStore,
  },
}

// Each index is within the immutable builder-checked range for this view.
#[allow(
  clippy::indexing_slicing,
  clippy::as_conversions,
  clippy::cast_possible_truncation,
  clippy::arithmetic_side_effects
)]
impl<'a> FormView<'a> {
  pub(super) const fn form(self) -> FormId {
    match self {
      #[cfg(test)]
      Self::Input(form) => form.form,
      Self::Packed { form, .. } => form.form,
    }
  }
  pub(super) const fn minimum_total(self) -> f64 {
    match self {
      #[cfg(test)]
      Self::Input(form) => form.minimum_total,
      Self::Packed { form, .. } => form.minimum_total,
    }
  }
  pub(super) const fn token_count(self) -> usize {
    match self {
      #[cfg(test)]
      Self::Input(form) => form.tokens.len(),
      Self::Packed { form, .. } => form.tokens_len as usize,
    }
  }
  pub(super) const fn join_count(self) -> usize {
    match self {
      #[cfg(test)]
      Self::Input(form) => form.joins.len(),
      Self::Packed { form, .. } => form.joins_len as usize,
    }
  }

  pub(super) fn tokens(self) -> impl ExactSizeIterator<Item = AliasToken> + 'a {
    (0..self.token_count()).map(move |position| match self {
      #[cfg(test)]
      Self::Input(form) => form.tokens[position].clone(),
      Self::Packed { form, store } => {
        let weight = &store.weights[form.weights_start as usize + position];
        AliasToken {
          id: store.tokens[form.tokens_start as usize + position],
          weight: weight.value,
          minimum_weight: weight.minimum,
        }
      }
    })
  }

  pub(super) fn joins(self) -> impl ExactSizeIterator<Item = AliasJoin> + 'a {
    (0..self.join_count()).map(move |position| match self {
      #[cfg(test)]
      Self::Input(form) => form.joins[position].clone(),
      Self::Packed { form, store, .. } => {
        let (left, right) =
          store.positions[form.positions_start as usize + position];
        AliasJoin {
          id: store.joins[form.joins_start as usize + position],
          left,
          right,
        }
      }
    })
  }
}

#[cfg(test)]
#[path = "packed_forward_tests.rs"]
mod tests;
