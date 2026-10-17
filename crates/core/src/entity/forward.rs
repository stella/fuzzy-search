//! Validated caller-owned forward positions and posting projections.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::lexicon::Lexicon;
use super::postings::PostingBuilder;
use super::search::PostingList;
use super::{AliasId, AliasInput, EntityIndexOptions};
use crate::{Error, Result};

fn invalid(message: &str) -> Error {
  Error::from_reason(message)
}

pub(super) fn validate_alias(
  alias: &AliasInput,
  vocabularies: &[Vec<String>],
) -> Result<()> {
  if !alias.quality.is_finite()
    || !(0.0..=1.0).contains(&alias.quality)
    || alias.forms.is_empty()
  {
    return Err(invalid("Invalid entity alias quality or forms"));
  }
  let mut seen_forms = BTreeSet::new();
  let count = alias.forms.first().map_or(0, |form| form.tokens.len());
  if count == 0 || u32::try_from(count).is_err() {
    return Err(invalid("Entity alias requires bounded token positions"));
  }
  for form in &alias.forms {
    for token in &form.tokens {
      super::validate_weight(token.weight)?;
      super::validate_weight(token.minimum_weight)?;
    }
    let vocabulary = usize::try_from(form.form)
      .ok()
      .and_then(|index| vocabularies.get(index))
      .ok_or_else(|| invalid("Invalid entity alias form"))?;
    if !seen_forms.insert(form.form) || form.tokens.len() != count {
      return Err(invalid(
        "Entity alias forms must have distinct ids and equal positions",
      ));
    }
    let reference = alias
      .forms
      .first()
      .ok_or_else(|| invalid("Missing entity alias form"))?;
    if form
      .tokens
      .iter()
      .zip(&reference.tokens)
      .any(|(token, reference)| {
        !token.weight.total_cmp(&reference.weight).is_eq()
          || !token
            .minimum_weight
            .total_cmp(&reference.minimum_weight)
            .is_eq()
      })
    {
      return Err(invalid(
        "Entity alias forms must share position weights and minimum weights",
      ));
    }
    let mut minimum_total = 0.0;
    let mut weight_total = 0.0;
    for token in &form.tokens {
      if usize::try_from(token.id)
        .ok()
        .and_then(|id| vocabulary.get(id))
        .is_none()
        || token.minimum_weight > token.weight
      {
        return Err(invalid("Invalid entity alias token or weight"));
      }
      minimum_total += token.minimum_weight;
      weight_total += token.weight;
    }
    if !minimum_total.is_finite()
      || !weight_total.is_finite()
      || !form.minimum_total.is_finite()
      || form.minimum_total <= 0.0
      || form.minimum_total > minimum_total
      || !(weight_total / form.minimum_total).is_finite()
    {
      return Err(invalid("Invalid entity alias minimum total"));
    }
    let mut joins = BTreeSet::new();
    for join in &form.joins {
      let left = usize::try_from(join.left)
        .map_err(|_| invalid("Invalid entity join"))?;
      let right = usize::try_from(join.right)
        .map_err(|_| invalid("Invalid entity join"))?;
      if left >= count
        || right >= count
        || usize::try_from(join.id)
          .ok()
          .and_then(|id| vocabulary.get(id))
          .is_none()
        || !joins.insert((join.left, join.right, join.id))
      {
        return Err(invalid("Invalid entity join id or positions"));
      }
    }
  }
  Ok(())
}

// Bounds are directed upward before quantization; this also avoids multiplying
// a subnormal position weight by similarity before normalizing it.
fn upward_share(weight: f64, total: f64) -> Result<f64> {
  let share = (weight / total).next_up();
  if !share.is_finite() {
    return Err(invalid("Entity position share exceeds finite range"));
  }
  Ok(share)
}

fn upward_scaled(share: f64, similarity: f64) -> Result<f64> {
  if similarity == 0.0 {
    return Ok(0.0);
  }
  let impact = (share * similarity).next_up();
  if !impact.is_finite() {
    return Err(invalid("Entity initial impact exceeds finite range"));
  }
  Ok(impact)
}

pub(super) fn post_alias(
  postings: &mut PostingBuilder,
  id: AliasId,
  alias: &AliasInput,
  lexicons: &[Arc<Lexicon>],
  options: &EntityIndexOptions,
) -> Result<()> {
  for form in &alias.forms {
    let lexicon = usize::try_from(form.form)
      .ok()
      .and_then(|index| lexicons.get(index))
      .ok_or_else(|| invalid("Missing entity forward lexicon"))?;
    for token in &form.tokens {
      let text = lexicon
        .token(token.id)
        .ok_or_else(|| invalid("Missing entity forward token"))?;
      let share = upward_share(token.weight, form.minimum_total)?;
      postings.add(
        PostingList::Token {
          form: form.form,
          id: token.id,
        },
        id,
        share,
      )?;
      let prefix = text
        .chars()
        .next()
        .ok_or_else(|| invalid("Empty entity forward token"))?;
      let scalar_initial = text.chars().nth(1).is_none();
      let listed_initial = scalar_initial && prefix.len_utf16() == 1;
      let similarity = if scalar_initial {
        1.0
      } else {
        options.initial_similarity
      };
      postings.add(
        PostingList::QueryInitial {
          form: form.form,
          prefix,
        },
        id,
        upward_scaled(share, similarity)?,
      )?;
      if listed_initial {
        let utf16_prefix = text
          .encode_utf16()
          .next()
          .ok_or_else(|| invalid("Empty entity listed initial"))?;
        postings.add(
          PostingList::ListedInitial {
            form: form.form,
            prefix: utf16_prefix,
          },
          id,
          upward_scaled(share, options.initial_similarity)?,
        )?;
      }
    }
    for join in &form.joins {
      let left = usize::try_from(join.left)
        .ok()
        .and_then(|index| form.tokens.get(index))
        .ok_or_else(|| invalid("Missing entity join left position"))?;
      let right = usize::try_from(join.right)
        .ok()
        .and_then(|index| form.tokens.get(index))
        .ok_or_else(|| invalid("Missing entity join right position"))?;
      postings.add(
        PostingList::Join {
          form: form.form,
          id: join.id,
        },
        id,
        (upward_share(left.weight, form.minimum_total)?
          + upward_share(right.weight, form.minimum_total)?)
        .next_up(),
      )?;
    }
  }
  Ok(())
}
