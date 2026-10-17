//! Complete threshold retrieval over impact-ordered postings.
//!
//! Every unseen alias is bounded by the sum of the current list heads. On
//! first sight, forward data supplies the tighter per-position maxima bound.
//! Stopping is strict: an alias exactly at the threshold must survive.

use std::collections::{BTreeMap, BTreeSet};

use super::cost::Work;
use super::lexicon::Lexicon;
use super::packed_forward::{AliasView, FormView};
use super::{
  AliasId, Cost, EntityIndexOptions, EntityQuery, FormId, SearchParams,
  SearchResult, TokenId, UnitMatch,
};
use crate::{Error, Result};

/// Persisted weight limits; normalization remains caller-owned.
pub(super) const MIN_WEIGHT: f64 = 1e-150;
pub(super) const MAX_WEIGHT: f64 = 1e150;
const MIN_SUBNORMAL: f64 = f64::from_bits(1);
const ROUNDING_FACTOR: f64 = 1.0 + 4.0 * f64::EPSILON;

// Bound arithmetic includes relative rounding and absolute underflow error.
// Infinity deliberately widens a bound; zero operands stay exactly zero.
fn upward(value: f64) -> f64 {
  value.mul_add(ROUNDING_FACTOR, MIN_SUBNORMAL).next_up()
}

fn upper_product(left: f64, right: f64) -> f64 {
  if left == 0.0 || right == 0.0 {
    return 0.0;
  }
  upward(left * right)
}

fn upper_add(left: f64, right: f64) -> f64 {
  if left == 0.0 && right == 0.0 {
    return 0.0;
  }
  upward(left + right)
}

fn upper_sum(values: impl Iterator<Item = f64>) -> f64 {
  values.fold(0.0, upper_add)
}

fn upper_ratio(numerator: f64, denominator: f64) -> f64 {
  if numerator == 0.0 {
    return 0.0;
  }
  upward(numerator / denominator)
}

fn weighted_share(
  weights: impl Iterator<Item = (f64, f64)>,
  total: f64,
) -> f64 {
  upper_ratio(
    upper_sum(
      weights.map(|(weight, similarity)| upper_product(weight, similarity)),
    ),
    total,
  )
  .min(1.0)
}

fn geometric_ceiling(query: f64, listed: f64) -> f64 {
  upward(upper_product(query, listed).sqrt()).min(1.0)
}

// A posting group can combine many token positions, while the reference adds
// their rounded products sequentially. Account for every such addition even
// when a group is represented by just one cursor. Very large counts widen to
// infinity instead of truncating work or rounding the count downward.
fn summation_factor(positions: usize) -> f64 {
  let Ok(positions) = u32::try_from(positions) else {
    return f64::INFINITY;
  };
  let error = upper_product(f64::from(positions), 4.0 * f64::EPSILON);
  if error >= 1.0 {
    return f64::INFINITY;
  }
  upper_ratio(1.0, (1.0 - error).next_down())
}

/// Posting impacts must be rounded upward and sorted descending, alias id
/// ascending on ties. Initial lists use the query token's spelling form.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum PostingList {
  Token { form: FormId, id: TokenId },
  Join { form: FormId, id: TokenId },
  QueryInitial { form: FormId, prefix: char },
  ListedInitial { form: FormId, prefix: u16 },
}

/// Storage boundary: search does not depend on builder or serialization layout.
/// Aliases use dense insertion-order ids, validated weight bounds and quality
/// factors in [0, 1]. All spelling forms describe the same token positions.
/// Query weights are caller-prepared; search preserves their exact values.
pub(super) trait SearchIndex {
  fn options(&self) -> &EntityIndexOptions;
  fn alias_count(&self) -> usize;
  /// A positive lower bound for every listed position weight.
  fn minimum_token_weight(&self) -> f64;
  /// Cached maximum position count, for reference summation rounding.
  fn maximum_alias_tokens(&self) -> usize;
  fn alias(&self, id: AliasId) -> Option<AliasView<'_>>;
  fn lexicon(&self, form: FormId) -> Option<&Lexicon>;
  fn posting(&self, list: PostingList, offset: usize)
  -> Option<(AliasId, f64)>;
}

/// Invert the caller's active `moveToward` boosts, including their FP margin.
pub fn threshold(cutoff: f64, boost_product: f64) -> Result<f64> {
  if !cutoff.is_finite()
    || !(0.0..=1.0).contains(&cutoff)
    || !boost_product.is_finite()
    || !(0.0..=1.0).contains(&boost_product)
  {
    return Err(Error::from_reason("Invalid entity cutoff or boost product"));
  }
  if boost_product == 0.0 {
    return Ok(0.0);
  }
  Ok((1.0 - (1.0 - cutoff) / boost_product - 1e-9).max(0.0))
}

struct Cursor {
  list: PostingList,
  similarity: f64,
  offset: usize,
}

struct Prepared {
  matches: Vec<UnitMatch>,
  similarities: BTreeMap<(usize, FormId, TokenId), f64>,
  cursors: Vec<Cursor>,
  q_max: f64,
}

fn invalid(message: &str) -> Error {
  Error::from_reason(message)
}

fn count(value: usize) -> Result<u64> {
  u64::try_from(value).map_err(|_| invalid("Entity work count exceeds u64"))
}

fn validate(
  query: &EntityQuery,
  params: &SearchParams,
  forms: u32,
) -> Result<()> {
  if !params.tau.is_finite()
    || !params.quality.is_finite()
    || !(0.0..=1.0).contains(&params.quality)
  {
    return Err(invalid("Invalid entity search threshold or quality"));
  }
  for token in &query.tokens {
    if token.form >= forms
      || token.text.is_empty()
      || !token.weight.is_finite()
      || token.weight <= 0.0
    {
      return Err(invalid("Invalid entity query token"));
    }
  }
  if !query
    .tokens
    .iter()
    .map(|token| token.weight)
    .sum::<f64>()
    .is_finite()
  {
    return Err(invalid("Entity query total weight is not finite"));
  }
  for unit in &query.units {
    if unit.form >= forms || unit.text.is_empty() || unit.positions.is_empty() {
      return Err(invalid("Invalid entity query unit"));
    }
    let mut positions = BTreeSet::new();
    for position in &unit.positions {
      let position = usize::try_from(*position)
        .map_err(|_| invalid("Entity query position exceeds usize"))?;
      if position >= query.tokens.len() || !positions.insert(position) {
        return Err(invalid("Invalid or duplicate entity query position"));
      }
    }
  }
  for (position, token) in query.tokens.iter().enumerate() {
    if token.text.chars().count() == 1 {
      continue;
    }
    let position = u32::try_from(position)
      .map_err(|_| invalid("Entity query position exceeds u32"))?;
    if !query.units.iter().any(|unit| {
      unit.form == token.form
        && unit.text == token.text
        && unit.positions == [position]
    }) {
      return Err(invalid(
        "Entity query requires a single unit for every word",
      ));
    }
  }
  Ok(())
}

fn prepare(
  index: &impl SearchIndex,
  query: &EntityQuery,
  cost: &mut Cost,
  max_cost: u64,
) -> Result<(Prepared, bool)> {
  let mut prepared = Prepared {
    matches: Vec::new(),
    similarities: BTreeMap::new(),
    cursors: Vec::new(),
    q_max: 0.0,
  };
  let mut maxima = vec![0.0_f64; query.tokens.len()];
  for (unit_index, unit) in query.units.iter().enumerate() {
    let lexicon = index
      .lexicon(unit.form)
      .ok_or_else(|| invalid("Missing entity query lexicon"))?;
    let lookup =
      lexicon.lookup(&unit.text, &index.options().budget, cost, max_cost)?;
    for hit in lookup.matches {
      prepared
        .similarities
        .insert((unit_index, unit.form, hit.id), hit.similarity);
      prepared.matches.push(UnitMatch {
        unit: u32::try_from(unit_index)
          .map_err(|_| invalid("Entity unit exceeds u32"))?,
        form: unit.form,
        id: hit.id,
        similarity: hit.similarity,
      });
      for position in &unit.positions {
        let maximum = maxima
          .get_mut(
            usize::try_from(*position)
              .map_err(|_| invalid("Entity position exceeds usize"))?,
          )
          .ok_or_else(|| invalid("Missing entity query position"))?;
        *maximum = maximum.max(hit.similarity);
      }
      for list in [
        PostingList::Token {
          form: unit.form,
          id: hit.id,
        },
        PostingList::Join {
          form: unit.form,
          id: hit.id,
        },
      ] {
        prepared.cursors.push(Cursor {
          list,
          similarity: hit.similarity,
          offset: 0,
        });
      }
    }
    if lookup.exhausted {
      return Ok((prepared, true));
    }
  }
  add_initial_cursors(index, query, &mut maxima, &mut prepared.cursors)?;
  let total = query.tokens.iter().map(|token| token.weight).sum::<f64>();
  if total > 0.0 {
    prepared.q_max = weighted_share(
      query
        .tokens
        .iter()
        .zip(maxima)
        .map(|(token, similarity)| (token.weight, similarity)),
      total,
    );
  }
  Ok((prepared, false))
}

fn add_initial_cursors(
  index: &impl SearchIndex,
  query: &EntityQuery,
  maxima: &mut [f64],
  cursors: &mut Vec<Cursor>,
) -> Result<()> {
  for (position, token) in query.tokens.iter().enumerate() {
    let prefix = token
      .text
      .chars()
      .next()
      .ok_or_else(|| invalid("Missing entity initial"))?;
    let initial = token.text.chars().count() == 1;
    let list = if initial {
      PostingList::QueryInitial {
        form: token.form,
        prefix,
      }
    } else {
      let prefix_unit = token
        .text
        .encode_utf16()
        .next()
        .ok_or_else(|| invalid("Missing entity initial"))?;
      PostingList::ListedInitial {
        form: token.form,
        prefix: prefix_unit,
      }
    };
    // A query initial may match an identical listed initial with similarity 1.
    // Noninitial words may still match a listed initial without a lexicon hit.
    if index.posting(list, 0).is_some() {
      let maximum = maxima
        .get_mut(position)
        .ok_or_else(|| invalid("Missing entity query maximum"))?;
      *maximum = maximum.max(if initial {
        1.0
      } else {
        index.options().initial_similarity
      });
      cursors.push(Cursor {
        list,
        similarity: 1.0,
        offset: 0,
      });
    }
  }
  Ok(())
}

/// Random-access upper bound; matching positions can be reused here, unlike
/// the final scorer's greedy one-to-one alignment.
fn alias_bound(
  index: &impl SearchIndex,
  query: &EntityQuery,
  prepared: &Prepared,
  alias: AliasView<'_>,
) -> Result<f64> {
  let mut query_max = vec![0.0_f64; query.tokens.len()];
  let listed_count = alias.forms().next().map_or(0, FormView::token_count);
  let mut listed_max = vec![0.0_f64; listed_count];
  let mut anchored = false;
  let mut joined = false;
  for (unit_index, unit) in query.units.iter().enumerate() {
    let Some(form) = alias.forms().find(|form| form.form() == unit.form) else {
      continue;
    };
    for (position, token) in form.tokens().enumerate() {
      let similarity = prepared
        .similarities
        .get(&(unit_index, unit.form, token.id))
        .copied()
        .unwrap_or(0.0);
      anchored |= similarity > 0.0;
      update_maxima(
        &mut query_max,
        &mut listed_max,
        &unit.positions,
        &[position],
        similarity,
      )?;
    }
    for join in form.joins() {
      let similarity = prepared
        .similarities
        .get(&(unit_index, unit.form, join.id))
        .copied()
        .unwrap_or(0.0);
      let positions = [
        usize::try_from(join.left)
          .map_err(|_| invalid("Invalid entity join"))?,
        usize::try_from(join.right)
          .map_err(|_| invalid("Invalid entity join"))?,
      ];
      anchored |= similarity > 0.0;
      joined |= similarity > 0.0;
      update_maxima(
        &mut query_max,
        &mut listed_max,
        &unit.positions,
        &positions,
        similarity,
      )?;
    }
  }
  if !anchored {
    return Ok(0.0);
  }
  // Legacy's single-unit fast path deliberately omits initial coverage.
  // Alternate spelling units with the same position are one conceptual unit.
  if query.tokens.len() == 1 && !joined {
    return Ok(coverage_bound(query, alias, &query_max, &listed_max));
  }
  for (position, token) in query.tokens.iter().enumerate() {
    let Some(form) = alias.forms().find(|form| form.form() == token.form)
    else {
      continue;
    };
    for (listed, listed_token) in form.tokens().enumerate() {
      let text = index
        .lexicon(form.form())
        .and_then(|lexicon| lexicon.token(listed_token.id))
        .ok_or_else(|| invalid("Missing entity forward token"))?;
      let query_initial = token.text.chars().count() == 1;
      // Preserve JavaScript raw.length === 1 (UTF-16), including astral initials.
      let listed_initial = text.encode_utf16().count() == 1;
      let similarity = if query_initial && text.starts_with(&token.text) {
        if listed_initial {
          1.0
        } else {
          index.options().initial_similarity
        }
      } else if !query_initial && listed_initial && token.text.starts_with(text)
      {
        index.options().initial_similarity
      } else {
        0.0
      };
      update_maxima(
        &mut query_max,
        &mut listed_max,
        &[u32::try_from(position)
          .map_err(|_| invalid("Invalid entity position"))?],
        &[listed],
        similarity,
      )?;
    }
  }
  Ok(coverage_bound(query, alias, &query_max, &listed_max))
}

fn coverage_bound(
  query: &EntityQuery,
  alias: AliasView<'_>,
  query_max: &[f64],
  listed_max: &[f64],
) -> f64 {
  // Forms describe the same positions and weights. Taking the maximum also
  // safely handles caller-supplied form-specific weight bounds.
  let mut listed_share = 0.0_f64;
  for form in alias.forms() {
    let explained = upper_sum(
      form
        .tokens()
        .zip(listed_max)
        .map(|(token, similarity)| upper_product(token.weight, *similarity)),
    );
    listed_share =
      listed_share.max(upper_ratio(explained, form.minimum_total()).min(1.0));
  }
  let total = query.tokens.iter().map(|token| token.weight).sum::<f64>();
  if total == 0.0 {
    return 0.0;
  }
  let query_share = weighted_share(
    query
      .tokens
      .iter()
      .zip(query_max)
      .map(|(token, similarity)| (token.weight, *similarity)),
    total,
  );
  geometric_ceiling(query_share, listed_share)
}

fn update_maxima(
  query: &mut [f64],
  listed: &mut [f64],
  query_positions: &[u32],
  listed_positions: &[usize],
  similarity: f64,
) -> Result<()> {
  for position in query_positions {
    let maximum = query
      .get_mut(
        usize::try_from(*position)
          .map_err(|_| invalid("Invalid entity query position"))?,
      )
      .ok_or_else(|| invalid("Missing entity query position"))?;
    *maximum = maximum.max(similarity);
  }
  for position in listed_positions {
    let maximum = listed
      .get_mut(*position)
      .ok_or_else(|| invalid("Missing entity listed position"))?;
    *maximum = maximum.max(similarity);
  }
  Ok(())
}

fn exact(
  index: &impl SearchIndex,
  query: &EntityQuery,
  alias: AliasView<'_>,
) -> Result<bool> {
  let mut query_text = query
    .tokens
    .iter()
    .map(|token| token.text.as_str())
    .collect::<Vec<_>>();
  query_text.sort_unstable();
  // The raw form is identified by the query tokens, not inferred from form id.
  let Some(first_token) = query.tokens.first() else {
    return Ok(false);
  };
  if query
    .tokens
    .iter()
    .any(|other| other.form != first_token.form)
  {
    return Ok(false);
  }
  let Some(form) = alias.forms().find(|form| form.form() == first_token.form)
  else {
    return Ok(false);
  };
  let mut listed_text = form
    .tokens()
    .map(|token| {
      index
        .lexicon(form.form())
        .and_then(|lexicon| lexicon.token(token.id))
        .ok_or_else(|| invalid("Missing entity exact token"))
    })
    .collect::<Result<Vec<_>>>()?;
  listed_text.sort_unstable();
  Ok(query_text == listed_text)
}

fn random_access_work(
  query: &EntityQuery,
  alias: AliasView<'_>,
) -> Result<u64> {
  // Each spelling unit compares only with its corresponding alias form.
  let mut pairs = 0_u64;
  for unit in &query.units {
    if let Some(form) = alias.forms().find(|form| form.form() == unit.form) {
      pairs = pairs
        .checked_add(count(form.token_count())?)
        .and_then(|pairs| pairs.checked_add(count(form.join_count()).ok()?))
        .ok_or_else(|| invalid("Entity work count overflow"))?;
    }
  }
  for token in &query.tokens {
    if let Some(form) = alias.forms().find(|form| form.form() == token.form) {
      pairs = pairs
        .checked_add(count(form.token_count())?)
        .ok_or_else(|| invalid("Entity work count overflow"))?;
    }
  }
  Ok(pairs)
}

fn initial_rounding_factor(index: &impl SearchIndex) -> f64 {
  let similarity = index.options().initial_similarity;
  if similarity == 0.0 {
    return 1.0;
  }
  // Initial impacts already contain their similarity. Use a lower product in
  // this denominator so rounding cannot understate their absolute error.
  let denominator = (index.minimum_token_weight() * similarity).next_down();
  if denominator <= 0.0 {
    return f64::INFINITY;
  }
  upper_add(1.0, upper_ratio(MIN_SUBNORMAL, denominator))
}

fn frontier_bound(index: &impl SearchIndex, cursors: &[Cursor]) -> f64 {
  let product_error = upper_ratio(MIN_SUBNORMAL, index.minimum_token_weight());
  let initial_factor = initial_rounding_factor(index);
  let frontier = upper_sum(cursors.iter().filter_map(|cursor| {
    index
      .posting(cursor.list, cursor.offset)
      .map(|(_, impact)| match cursor.list {
        PostingList::Token { .. } | PostingList::Join { .. } => {
          let similarity = upper_add(cursor.similarity, product_error).min(1.0);
          upper_product(similarity, impact)
        }
        PostingList::QueryInitial { .. }
        | PostingList::ListedInitial { .. } => upper_product(
          cursor.similarity,
          upper_product(impact, initial_factor),
        ),
      })
  }));
  upper_product(frontier, summation_factor(index.maximum_alias_tokens()))
}

pub(super) fn search(
  index: &impl SearchIndex,
  query: &EntityQuery,
  params: &SearchParams,
) -> Result<SearchResult> {
  validate(query, params, index.options().forms)?;
  let mut cost = Cost::default();
  let (prepared, mut exhausted) =
    prepare(index, query, &mut cost, params.max_cost)?;
  let mut survivors = Vec::new();
  let mut seen = BTreeSet::new();
  let mut cursors = prepared
    .cursors
    .iter()
    .map(|cursor| Cursor { ..*cursor })
    .collect::<Vec<_>>();
  if params.tau <= 0.0 && !exhausted {
    // At zero threshold even disjoint aliases qualify. There is no posting
    // discovery proof for zero-score aliases, so enumerate the forward store.
    for id in 0..index.alias_count() {
      if !cost.charge(Work::Survivor, params.max_cost) {
        exhausted = true;
        break;
      }
      survivors.push(
        u32::try_from(id).map_err(|_| invalid("Entity alias exceeds u32"))?,
      );
    }
  } else if !exhausted && !query.tokens.is_empty() {
    'walk: loop {
      let frontier = frontier_bound(index, &cursors);
      let ceiling = upper_product(
        geometric_ceiling(prepared.q_max, frontier.min(1.0)),
        params.quality,
      );
      if ceiling + 1e-12 < params.tau {
        break;
      }
      let mut advanced = false;
      for cursor in &mut cursors {
        let Some((id, _)) = index.posting(cursor.list, cursor.offset) else {
          continue;
        };
        if !cost.charge(Work::SortedAccess, params.max_cost) {
          exhausted = true;
          break 'walk;
        }
        cursor.offset = cursor
          .offset
          .checked_add(1)
          .ok_or_else(|| invalid("Entity posting offset exceeds usize"))?;
        advanced = true;
        if !seen.insert(id) {
          continue;
        }
        let alias = index
          .alias(id)
          .ok_or_else(|| invalid("Missing entity posting alias"))?;
        let pairs = random_access_work(query, alias)?;
        if !cost.charge(Work::RandomAccess(pairs), params.max_cost) {
          exhausted = true;
          break 'walk;
        }
        let bound = if exact(index, query, alias)? {
          1.0
        } else {
          alias_bound(index, query, &prepared, alias)?
        };
        if upper_product(upper_product(bound, alias.quality()), params.quality)
          + 1e-12
          >= params.tau
        {
          if !cost.charge(Work::Survivor, params.max_cost) {
            exhausted = true;
            break 'walk;
          }
          survivors.push(id);
        }
      }
      if !advanced {
        break;
      }
    }
  }
  survivors.sort_unstable();
  Ok(SearchResult {
    unit_matches: prepared.matches,
    query_weights: query.tokens.iter().map(|token| token.weight).collect(),
    survivors,
    cost,
    exhausted,
  })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
  use super::super::oracle;
  use super::super::{
    AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
    EntityIndex, EntityIndexBuilder, QueryToken, QueryUnit,
  };
  use super::*;
  use proptest::prelude::*;

  struct Fixture {
    options: EntityIndexOptions,
    lexicon: Lexicon,
    aliases: Vec<AliasInput>,
    lists: BTreeMap<PostingList, Vec<(AliasId, f64)>>,
  }

  impl SearchIndex for Fixture {
    fn options(&self) -> &EntityIndexOptions {
      &self.options
    }
    fn alias_count(&self) -> usize {
      self.aliases.len()
    }
    fn minimum_token_weight(&self) -> f64 {
      self
        .aliases
        .iter()
        .flat_map(|alias| &alias.forms)
        .flat_map(|form| &form.tokens)
        .map(|token| token.weight)
        .fold(MAX_WEIGHT, f64::min)
    }
    fn maximum_alias_tokens(&self) -> usize {
      self
        .aliases
        .iter()
        .flat_map(|alias| &alias.forms)
        .map(|form| form.tokens.len())
        .max()
        .unwrap_or(0)
    }
    fn alias(&self, id: AliasId) -> Option<AliasView<'_>> {
      self
        .aliases
        .get(usize::try_from(id).ok()?)
        .map(AliasView::from)
    }
    fn lexicon(&self, form: FormId) -> Option<&Lexicon> {
      (form < self.options.forms).then_some(&self.lexicon)
    }
    fn posting(
      &self,
      list: PostingList,
      offset: usize,
    ) -> Option<(AliasId, f64)> {
      self.lists.get(&list)?.get(offset).copied()
    }
  }

  const WORDS: [&str; 12] = [
    "a",
    "alpha",
    "alhpa",
    "beta",
    "betas",
    "日",
    "日本",
    "𐐀",
    "𐐀word",
    "علي",
    "alphabeta",
    "2026",
  ];

  impl Fixture {
    fn new(rows: &[Vec<(u8, u16, u8)>], joins: bool, weak: bool) -> Self {
      let mut fixture = Self {
        options: EntityIndexOptions {
          forms: 1,
          budget: EditBudget {
            by_length: vec![0, 0, 0, 0, 1, 1, 2],
            basis: BudgetBasis::Shorter,
          },
          initial_weight: 0.25,
          initial_similarity: 0.8,
        },
        lexicon: Lexicon::new(&WORDS.map(str::to_owned)).unwrap(),
        aliases: Vec::new(),
        lists: BTreeMap::new(),
      };
      for (alias_id, row) in rows.iter().enumerate() {
        let tokens = row
          .iter()
          .map(|&(id, weight, discount)| AliasToken {
            id: u32::from(id) % 12,
            weight: f64::from(weight),
            minimum_weight: f64::from(weight) * (f64::from(discount) + 1.0)
              / 256.0,
          })
          .collect::<Vec<_>>();
        let minimum_total =
          tokens.iter().map(|token| token.minimum_weight).sum();
        let joins = if joins && tokens.len() >= 2 {
          vec![AliasJoin {
            id: 10,
            left: 0,
            right: 1,
          }]
        } else {
          vec![]
        };
        let form = AliasForm {
          form: 0,
          tokens,
          joins,
          minimum_total,
        };
        let impacts = alias_impacts(
          &fixture.lexicon,
          &form,
          fixture.options.initial_similarity,
        );
        for (list, impact) in impacts {
          fixture
            .lists
            .entry(list)
            .or_default()
            .push((u32::try_from(alias_id).unwrap(), impact));
        }
        fixture.aliases.push(AliasInput {
          entry_id: u32::try_from(alias_id).unwrap(),
          entity_group: 0,
          quality: if weak && alias_id % 2 == 0 { 0.9 } else { 1.0 },
          forms: vec![form],
        });
      }
      for list in fixture.lists.values_mut() {
        // Per-list scaling preserves ratios above one until after similarity.
        let maximum = list
          .iter()
          .map(|(_, impact)| *impact)
          .fold(0.0_f64, f64::max);
        let quantum = (maximum / f64::from(u16::MAX)).next_up();
        for (_, impact) in list.iter_mut() {
          let encoded = (*impact / quantum).ceil();
          assert!(encoded <= f64::from(u16::MAX));
          *impact = (encoded * quantum).next_up();
        }
        list.sort_by(|left, right| {
          right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
        });
      }
      fixture
    }
  }

  fn alias_impacts(
    lexicon: &Lexicon,
    form: &AliasForm,
    initial_similarity: f64,
  ) -> BTreeMap<PostingList, f64> {
    let mut impacts = BTreeMap::new();
    for token in &form.tokens {
      *impacts
        .entry(PostingList::Token {
          form: 0,
          id: token.id,
        })
        .or_insert(0.0) += token.weight / form.minimum_total;
      let text = lexicon.token(token.id).unwrap();
      let similarity = if text.chars().count() == 1 {
        1.0
      } else {
        initial_similarity
      };
      if similarity > 0.0 {
        let impact = impacts
          .entry(PostingList::QueryInitial {
            form: form.form,
            prefix: text.chars().next().unwrap(),
          })
          .or_insert(0.0);
        *impact = (token.weight / form.minimum_total)
          .mul_add(similarity, *impact)
          .next_up();
      }
      if text.encode_utf16().count() == 1 && initial_similarity > 0.0 {
        let impact = impacts
          .entry(PostingList::ListedInitial {
            form: form.form,
            prefix: text.encode_utf16().next().unwrap(),
          })
          .or_insert(0.0);
        *impact = (token.weight / form.minimum_total)
          .mul_add(initial_similarity, *impact)
          .next_up();
      }
    }
    for join in &form.joins {
      *impacts
        .entry(PostingList::Join {
          form: 0,
          id: join.id,
        })
        .or_insert(0.0) += (form
        .tokens
        .get(usize::try_from(join.left).unwrap())
        .unwrap()
        .weight
        + form
          .tokens
          .get(usize::try_from(join.right).unwrap())
          .unwrap()
          .weight)
        / form.minimum_total;
    }
    impacts
  }

  fn word(id: u8) -> &'static str {
    WORDS
      .get(usize::from(id).checked_rem(WORDS.len()).unwrap())
      .copied()
      .unwrap()
  }

  fn query(ids: &[u8], joins: bool) -> EntityQuery {
    let tokens = ids
      .iter()
      .map(|id| QueryToken {
        form: 0,
        text: word(*id).to_owned(),
        weight: 1.0 + f64::from(*id),
      })
      .collect::<Vec<_>>();
    let mut units = tokens
      .iter()
      .enumerate()
      .filter(|(_, token)| token.text.chars().count() != 1)
      .map(|(position, token)| QueryUnit {
        form: 0,
        text: token.text.clone(),
        positions: vec![u32::try_from(position).unwrap()],
      })
      .collect::<Vec<_>>();
    if joins && tokens.len() >= 2 {
      units.push(QueryUnit {
        form: 0,
        text: "alphabeta".to_owned(),
        positions: vec![0, 1],
      });
    }
    EntityQuery { tokens, units }
  }

  fn check_oracle(fixture: &Fixture, query: &EntityQuery, cutoff: f64) {
    check_oracle_with_quality(fixture, query, cutoff, 1.0);
  }

  fn check_oracle_with_quality(
    fixture: &Fixture,
    query: &EntityQuery,
    cutoff: f64,
    quality: f64,
  ) {
    let params = SearchParams {
      tau: cutoff,
      quality,
      max_cost: u64::MAX,
    };
    let result = search(fixture, query, &params).unwrap();
    assert!(!result.exhausted);
    let repeated = search(fixture, query, &params).unwrap();
    assert_eq!(result, repeated);
    let (prepared, exhausted) =
      prepare(fixture, query, &mut Cost::default(), u64::MAX).unwrap();
    assert!(!exhausted);
    let weights = query
      .tokens
      .iter()
      .map(|token| token.weight)
      .collect::<Vec<_>>();
    for (id, alias) in fixture.aliases.iter().enumerate() {
      let pairs = oracle::candidate_pairs(
        query,
        alias,
        &result.unit_matches,
        |_, token_id| fixture.lexicon.token(token_id).unwrap().to_owned(),
        fixture.options.initial_similarity,
      );
      let form = alias.forms.first().unwrap();
      let listed = form
        .tokens
        .iter()
        .map(|token| token.weight)
        .collect::<Vec<_>>();
      let minimum = form
        .tokens
        .iter()
        .map(|token| token.minimum_weight)
        .collect::<Vec<_>>();
      let mut raw_query = query
        .tokens
        .iter()
        .map(|token| token.text.clone())
        .collect::<Vec<_>>();
      raw_query.sort();
      let mut raw_listed = form
        .tokens
        .iter()
        .map(|token| fixture.lexicon.token(token.id).unwrap().to_owned())
        .collect::<Vec<_>>();
      raw_listed.sort();
      let is_exact = raw_query == raw_listed;
      assert_eq!(exact(fixture, query, alias.into()).unwrap(), is_exact);
      let score = if is_exact {
        alias.quality
      } else {
        oracle::score(&pairs, &weights, &listed, &minimum, alias.quality)
      };
      let bound = alias_bound(fixture, query, &prepared, alias.into()).unwrap()
        * alias.quality;
      let estimate = oracle::candidate_estimate(
        query,
        alias,
        &result.unit_matches,
        |_, token_id| fixture.lexicon.token(token_id).unwrap().to_owned(),
        fixture.options.initial_similarity,
        alias.quality,
      );
      assert!(
        bound + 1e-12 >= estimate,
        "bound {bound} < estimate {estimate}"
      );
      if !is_exact {
        assert!(bound + 1e-12 >= score, "bound {bound} < score {score}");
      }
      if score * quality >= cutoff {
        assert!(
          result.survivors.contains(&u32::try_from(id).unwrap()),
          "lost alias {id}: score {score}, cutoff {cutoff}, query {query:?}"
        );
      }
    }
  }

  fn rescale_fixture(fixture: &mut Fixture, scale: f64) {
    fixture.lists.clear();
    for (alias_id, alias) in fixture.aliases.iter_mut().enumerate() {
      for form in &mut alias.forms {
        for token in &mut form.tokens {
          token.weight = scale;
          token.minimum_weight = scale;
        }
        form.minimum_total =
          form.tokens.iter().map(|token| token.minimum_weight).sum();
        for (list, impact) in alias_impacts(
          &fixture.lexicon,
          form,
          fixture.options.initial_similarity,
        ) {
          fixture
            .lists
            .entry(list)
            .or_default()
            .push((u32::try_from(alias_id).unwrap(), impact.next_up()));
        }
      }
    }
    for list in fixture.lists.values_mut() {
      list.sort_by(|left, right| {
        right
          .1
          .total_cmp(&left.1)
          .then_with(|| left.0.cmp(&right.0))
      });
    }
  }

  fn assert_extreme_bounds(fixture: &Fixture, query: &EntityQuery) {
    let (prepared, exhausted) =
      prepare(fixture, query, &mut Cost::default(), u64::MAX).unwrap();
    assert!(!exhausted);
    let weights = query
      .tokens
      .iter()
      .map(|token| token.weight)
      .collect::<Vec<_>>();
    let scores = fixture
      .aliases
      .iter()
      .map(|alias| {
        let pairs = oracle::candidate_pairs(
          query,
          alias,
          &prepared.matches,
          |_, token_id| fixture.lexicon.token(token_id).unwrap().to_owned(),
          fixture.options.initial_similarity,
        );
        let form = alias.forms.first().unwrap();
        let listed = form
          .tokens
          .iter()
          .map(|token| token.weight)
          .collect::<Vec<_>>();
        let minimum = form
          .tokens
          .iter()
          .map(|token| token.minimum_weight)
          .collect::<Vec<_>>();
        let score = if exact(fixture, query, alias.into()).unwrap() {
          alias.quality
        } else {
          oracle::score(&pairs, &weights, &listed, &minimum, alias.quality)
        };
        let bound = if exact(fixture, query, alias.into()).unwrap() {
          1.0
        } else {
          alias_bound(fixture, query, &prepared, alias.into()).unwrap()
        };
        assert!(
          upper_product(bound, alias.quality) >= score,
          "bound {bound} below oracle {score}"
        );
        score
      })
      .collect::<Vec<_>>();
    let mut cursors = prepared.cursors;
    let mut seen = BTreeSet::new();
    loop {
      let ceiling = geometric_ceiling(
        prepared.q_max,
        frontier_bound(fixture, &cursors).min(1.0),
      );
      for (alias_id, score) in scores.iter().enumerate() {
        if !seen.contains(&u32::try_from(alias_id).unwrap()) {
          assert!(
            ceiling >= *score,
            "frontier {ceiling} below unseen oracle {score}"
          );
        }
      }
      let mut advanced = false;
      for cursor in &mut cursors {
        if let Some((alias_id, _)) = fixture.posting(cursor.list, cursor.offset)
        {
          seen.insert(alias_id);
          cursor.offset = cursor.offset.checked_add(1).unwrap();
          advanced = true;
        }
      }
      if !advanced {
        break;
      }
    }
  }

  fn extreme_weight() -> impl Strategy<Value = f64> {
    prop_oneof![
      (1_u64..=2048).prop_map(f64::from_bits),
      (-300_i32..=300).prop_map(|exponent| 10.0_f64.powi(exponent)),
      Just(MIN_WEIGHT),
      Just(MAX_WEIGHT),
      Just(1.0),
    ]
  }

  fn real_builder(fixture: &Fixture) -> EntityIndexBuilder {
    let mut builder = EntityIndexBuilder::new(fixture.options.clone()).unwrap();
    for form in 0..fixture.options.forms {
      for (id, text) in WORDS.iter().enumerate() {
        assert_eq!(
          builder.intern(form, text).unwrap(),
          u32::try_from(id).unwrap()
        );
      }
    }
    for (id, alias) in fixture.aliases.iter().enumerate() {
      assert_eq!(
        builder.add_alias(alias.clone()).unwrap(),
        u32::try_from(id).unwrap()
      );
    }
    builder
  }

  fn real_index(fixture: &Fixture, step: Option<u32>) -> EntityIndex {
    let mut builder = real_builder(fixture);
    if let Some(maximum) = step {
      while !builder.finish_step(maximum).unwrap() {}
    }
    builder
      .finish(vec![
        vec![1.0; WORDS.len()];
        usize::try_from(fixture.options.forms).unwrap()
      ])
      .unwrap()
  }

  fn assert_real_roundtrips(
    fixture: &Fixture,
    query: &EntityQuery,
    params: &SearchParams,
  ) {
    let index = real_index(fixture, None);
    let expected = index.search(query, params).unwrap();
    for step in [1, 3, 31] {
      let stepped = real_index(fixture, Some(step));
      assert_eq!(expected, stepped.search(query, params).unwrap());
    }
    let restored = EntityIndex::from_bytes(&index.to_bytes().unwrap()).unwrap();
    assert_eq!(expected, restored.search(query, params).unwrap());
    let full = index
      .search(
        query,
        &SearchParams {
          max_cost: u64::MAX,
          ..*params
        },
      )
      .unwrap();
    assert!(!full.exhausted);
    let weights = query
      .tokens
      .iter()
      .map(|token| token.weight)
      .collect::<Vec<_>>();
    for (alias_id, alias) in fixture.aliases.iter().enumerate() {
      let pairs = oracle::candidate_pairs(
        query,
        alias,
        &full.unit_matches,
        |_, token_id| fixture.lexicon.token(token_id).unwrap().to_owned(),
        fixture.options.initial_similarity,
      );
      let form = alias.forms.first().unwrap();
      let listed = form
        .tokens
        .iter()
        .map(|token| token.weight)
        .collect::<Vec<_>>();
      let minimum = form
        .tokens
        .iter()
        .map(|token| token.minimum_weight)
        .collect::<Vec<_>>();
      let score = if exact(fixture, query, alias.into()).unwrap() {
        alias.quality
      } else {
        oracle::score(&pairs, &weights, &listed, &minimum, alias.quality)
      };
      if score * params.quality >= params.tau {
        assert!(
          full.survivors.contains(&u32::try_from(alias_id).unwrap()),
          "real storage lost alias {alias_id} at score {score}"
        );
      }
    }
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn extreme_weight_bounds_and_frontier_preserve_every_oracle_match(
      rows in prop::collection::vec(prop::collection::vec((any::<u8>(), 1_u16..1000, 255_u8..=255), 1..7), 1..20),
      ids in prop::collection::vec(any::<u8>(), 1..6),
      query_scale in extreme_weight(), alias_scale in extreme_weight(),
      cutoff in 0_u16..=1000, joins in any::<bool>(), weak in any::<bool>(),
      initial_similarity in prop::sample::select(vec![0.0, 0.8, 1.0, MIN_SUBNORMAL, 1e-300]),
    ) {
      let mut fixture = Fixture::new(&rows, joins, weak);
      fixture.options.initial_similarity = initial_similarity;
      rescale_fixture(&mut fixture, alias_scale);
      let mut query = query(&ids, joins);
      for token in &mut query.tokens { token.weight = query_scale; }
      check_oracle(&fixture, &query, f64::from(cutoff) / 1000.0);
      assert_extreme_bounds(&fixture, &query);
    }

    #[test]
    fn real_storage_preserves_exact_aliases_for_every_scalar_initial(
      characters in prop::collection::vec(any::<char>(), 1..5),
    ) {
      let fixture = Fixture::new(&[], false, false);
      let mut builder = EntityIndexBuilder::new(fixture.options).unwrap();
      let texts = characters.into_iter().map(|character| character.to_string()).collect::<Vec<_>>();
      let ids = texts.iter().map(|text| builder.intern(0, text).unwrap()).collect::<Vec<_>>();
      let minimum_total = f64::from(u32::try_from(ids.len()).unwrap());
      builder.add_alias(AliasInput { entry_id: 0, entity_group: 0, quality: 1.0,
        forms: vec![AliasForm { form: 0, tokens: ids.into_iter().map(|id| AliasToken {
          id, weight: 1.0, minimum_weight: 1.0,
        }).collect(), joins: vec![], minimum_total }],
      }).unwrap();
      let vocabulary_size = texts.iter().collect::<BTreeSet<_>>().len();
      let index = builder.finish(vec![vec![1.0; vocabulary_size]]).unwrap();
      let query = EntityQuery { tokens: texts.into_iter().map(|text| QueryToken {
        form: 0, text, weight: 1.0,
      }).collect(), units: vec![] };
      let result = index.search(&query, &SearchParams { tau: 1.0, quality: 1.0, max_cost: u64::MAX }).unwrap();
      prop_assert!(!result.exhausted);
      prop_assert_eq!(result.survivors, [0]);
    }

    #[test]
    fn real_storage_finish_steps_and_serialization_preserve_results_and_cost(
      rows in prop::collection::vec(prop::collection::vec((any::<u8>(), 1_u16..1000, any::<u8>()), 1..7), 0..20),
      ids in prop::collection::vec(any::<u8>(), 1..6),
      cutoff in 0_u16..=1000, quality in 0_u16..=1000,
      joins in any::<bool>(), weak in any::<bool>(), cap in 0_u64..2000,
    ) {
      let fixture = Fixture::new(&rows, joins, weak);
      let query = query(&ids, joins);
      assert_real_roundtrips(&fixture, &query, &SearchParams {
        tau: f64::from(cutoff) / 1000.0, quality: f64::from(quality) / 1000.0, max_cost: cap,
      });
    }

    #[test]
    fn complete_against_brute_force_with_deterministic_cost(
      rows in prop::collection::vec(prop::collection::vec((any::<u8>(), 1_u16..1000, any::<u8>()), 1..7), 0..30),
      ids in prop::collection::vec(any::<u8>(), 1..6),
      cutoff in 0_u16..=1000, quality in 0_u16..=1000,
      joins in any::<bool>(), weak in any::<bool>(), cap in 0_u64..2000,
    ) {
      let fixture = Fixture::new(&rows, joins, weak);
      let query = query(&ids, joins);
      let quality = f64::from(quality) / 1000.0;
      check_oracle_with_quality(&fixture, &query, f64::from(cutoff) / 1000.0, quality);
      let params = SearchParams { tau: f64::from(cutoff) / 1000.0, quality, max_cost: cap };
      let bounded = search(&fixture, &query, &params).unwrap();
      prop_assert!(bounded.cost.total() <= cap);
      prop_assert_eq!(&bounded, &search(&fixture, &query, &params).unwrap());
      if !bounded.exhausted {
        prop_assert_eq!(bounded, search(&fixture, &query, &SearchParams { max_cost: u64::MAX, ..params }).unwrap());
      }
    }
  }

  #[test]
  fn alternate_spelling_forms_share_positions_and_combine_by_maximum() {
    let rows = vec![
      vec![(1, 1, 255), (3, 7, 255)],
      vec![(4, 4, 90), (11, 3, 255)],
    ];
    let alternate_rows = rows
      .iter()
      .map(|row| {
        row
          .iter()
          .map(|&(id, w, d)| ((id + 3) % 12, w, d))
          .collect()
      })
      .collect::<Vec<_>>();
    let mut fixture = Fixture::new(&rows, true, true);
    let alternate = Fixture::new(&alternate_rows, true, true);
    fixture.options.forms = 2;
    for (alias, alternate_alias) in
      fixture.aliases.iter_mut().zip(alternate.aliases)
    {
      let mut form = alternate_alias.forms.into_iter().next().unwrap();
      form.form = 1;
      alias.forms.push(form);
    }
    for (list, postings) in alternate.lists {
      let list = match list {
        PostingList::Token { id, .. } => PostingList::Token { form: 1, id },
        PostingList::Join { id, .. } => PostingList::Join { form: 1, id },
        PostingList::QueryInitial { prefix, .. } => {
          PostingList::QueryInitial { form: 1, prefix }
        }
        PostingList::ListedInitial { prefix, .. } => {
          PostingList::ListedInitial { form: 1, prefix }
        }
      };
      fixture.lists.insert(list, postings);
    }
    let mut query = query(&[1, 3], true);
    query.units.extend([
      QueryUnit {
        form: 1,
        text: WORDS[4].to_owned(),
        positions: vec![0],
      },
      QueryUnit {
        form: 1,
        text: WORDS[6].to_owned(),
        positions: vec![1],
      },
    ]);
    for cutoff in [0.0, 0.2, 0.6, 0.9, 1.0] {
      check_oracle(&fixture, &query, cutoff);
      check_oracle_with_quality(&fixture, &query, cutoff, 0.7);
    }
  }

  #[test]
  fn initial_postings_cover_underflow_before_embedded_similarity() {
    let mut fixture =
      Fixture::new(&[vec![(0, 1, 255), (3, 1, 255)]], false, false);
    rescale_fixture(&mut fixture, MIN_SUBNORMAL);
    let mut query = query(&[1, 3], false);
    for token in &mut query.tokens {
      token.weight = MIN_SUBNORMAL;
    }
    let result = search(
      &fixture,
      &query,
      &SearchParams {
        tau: 1.0,
        quality: 1.0,
        max_cost: u64::MAX,
      },
    )
    .unwrap();
    assert_eq!(result.survivors, [0]);
    check_oracle(&fixture, &query, 1.0);
  }

  #[test]
  fn minimum_subnormal_products_cannot_hide_an_oracle_score_of_one() {
    let mut fixture = Fixture::new(&[vec![(1, 1, 255)]], false, false);
    rescale_fixture(&mut fixture, MIN_SUBNORMAL);
    let mut query = query(&[2], false);
    query.tokens.first_mut().unwrap().weight = MIN_SUBNORMAL;
    let params = SearchParams {
      tau: 0.9,
      quality: 1.0,
      max_cost: u64::MAX,
    };
    let result = search(&fixture, &query, &params).unwrap();
    let alias = fixture.aliases.first().unwrap();
    let pairs = oracle::candidate_pairs(
      &query,
      alias,
      &result.unit_matches,
      |_, token_id| fixture.lexicon.token(token_id).unwrap().to_owned(),
      0.8,
    );
    let score = oracle::score(
      &pairs,
      &[MIN_SUBNORMAL],
      &[MIN_SUBNORMAL],
      &[MIN_SUBNORMAL],
      1.0,
    );
    assert_eq!(score.to_bits(), 1.0_f64.to_bits());
    assert_eq!(result.survivors, [0]);
    check_oracle(&fixture, &query, params.tau);
  }

  #[test]
  fn no_candidate_cap_and_inclusive_cutoff() {
    let mut fixture =
      Fixture::new(&vec![vec![(1, 1, 255)]; 1200], false, false);
    fixture.options.budget.by_length = vec![0];
    let query = query(&[1], false);
    let result = search(
      &fixture,
      &query,
      &SearchParams {
        tau: 1.0,
        quality: 1.0,
        max_cost: u64::MAX,
      },
    )
    .unwrap();
    assert_eq!(result.survivors.len(), 1200);
    assert_eq!(
      result.cost,
      Cost {
        lexicon_cells: 0,
        lexicon_matches: 1,
        sorted_accesses: 1200,
        random_access_pairs: 2400,
        survivors: 1200,
      }
    );
    let exact_cap = SearchParams {
      tau: 1.0,
      quality: 1.0,
      max_cost: result.cost.total(),
    };
    assert_eq!(result, search(&fixture, &query, &exact_cap).unwrap());
    let capped = search(
      &fixture,
      &query,
      &SearchParams {
        max_cost: exact_cap.max_cost - 1,
        ..exact_cap
      },
    )
    .unwrap();
    assert!(capped.exhausted);
    assert_eq!(capped.survivors.len(), 1199);
    check_oracle(&fixture, &query, 1.0);
  }

  #[test]
  fn initial_lists_discover_aliases_without_word_hits_and_zero_enumerates() {
    let fixture = Fixture::new(
      &[
        vec![(0, 1, 255), (3, 1, 255)],
        vec![(7, 1, 255), (3, 1, 255)],
        vec![(6, 1, 255)],
      ],
      false,
      false,
    );
    for ids in [
      &[1, 3][..],
      &[8, 3][..],
      &[0, 3][..],
      &[7, 3][..],
      &[11][..],
    ] {
      check_oracle(&fixture, &query(ids, false), 0.75);
      check_oracle(&fixture, &query(ids, false), 0.0);
    }
  }

  #[test]
  fn stops_before_any_posting_when_frontier_is_too_small() {
    let mut fixture = Fixture::new(
      &[vec![(1, 1, 255), (3, 1, 255), (11, 1000, 255)]],
      false,
      false,
    );
    fixture.options.budget.by_length = vec![0];
    let result = search(
      &fixture,
      &query(&[1, 3], false),
      &SearchParams {
        tau: 0.8,
        quality: 1.0,
        max_cost: u64::MAX,
      },
    )
    .unwrap();
    assert!(result.survivors.is_empty());
    assert_eq!(result.cost.sorted_accesses, 0);
    assert_eq!(result.cost.random_access_pairs, 0);
  }

  #[test]
  fn impact_ratio_above_one_is_scaled_before_frontier_clamping() {
    let fixture = Fixture::new(&[vec![(1, 1, 25)]], false, false);
    let query = EntityQuery {
      tokens: vec![QueryToken {
        form: 0,
        text: "alphx".to_owned(),
        weight: 1.0,
      }],
      units: vec![QueryUnit {
        form: 0,
        text: "alphx".to_owned(),
        positions: vec![0],
      }],
    };
    let impact = fixture
      .posting(PostingList::Token { form: 0, id: 1 }, 0)
      .unwrap()
      .1;
    assert!(impact > 1.0);
    let result = search(
      &fixture,
      &query,
      &SearchParams {
        tau: 0.85,
        quality: 1.0,
        max_cost: u64::MAX,
      },
    )
    .unwrap();
    // The legacy optimistic estimate exceeds .85; clipping this impact to one
    // before multiplying by .8 would stop at a frontier ceiling of .8.
    assert_eq!(result.survivors, [0]);
    assert_eq!(result.cost.random_access_pairs, 2);
    check_oracle(&fixture, &query, 0.8);
  }

  #[test]
  fn frontier_stops_after_the_high_impact_prefix_at_every_scale() {
    let mut previous_cost = None;
    for scale in [1, 2, 5] {
      let mut rows = vec![vec![(1, 1, 255)]];
      rows.extend(vec![vec![(1, 1, 255), (11, 1000, 255)]; 200 * scale]);
      let mut fixture = Fixture::new(&rows, false, false);
      fixture.options.budget.by_length = vec![0];
      let query = query(&[1], false);
      let result = search(
        &fixture,
        &query,
        &SearchParams {
          tau: 0.9,
          quality: 1.0,
          max_cost: u64::MAX,
        },
      )
      .unwrap();
      assert_eq!(result.survivors, [0]);
      assert_eq!(result.cost.sorted_accesses, 1);
      assert_eq!(result.cost.random_access_pairs, 2);
      if let Some(previous) = previous_cost {
        assert_eq!(previous, result.cost);
      }
      previous_cost = Some(result.cost);
      check_oracle(&fixture, &query, 0.9);
    }
  }

  #[test]
  fn boost_inverse_and_invalid_queries() {
    assert!((threshold(0.8, 0.5).unwrap() - (0.6 - 1e-9)).abs() < 1e-12);
    assert_eq!(threshold(1.0, 0.0).unwrap().to_bits(), 0.0_f64.to_bits());
    assert!(threshold(f64::NAN, 1.0).is_err());
    assert!(threshold(0.5, -1.0).is_err());
    let fixture = Fixture::new(&[], false, false);
    let mut query = query(&[1], false);
    let incomplete = EntityQuery {
      tokens: query.tokens.clone(),
      units: vec![],
    };
    assert!(
      search(
        &fixture,
        &incomplete,
        &SearchParams {
          tau: 1.0,
          quality: 1.0,
          max_cost: 100
        }
      )
      .is_err()
    );
    query.units.first_mut().unwrap().positions = vec![7];
    assert!(
      search(
        &fixture,
        &query,
        &SearchParams {
          tau: 0.5,
          quality: 1.0,
          max_cost: 100
        }
      )
      .is_err()
    );
  }
}
