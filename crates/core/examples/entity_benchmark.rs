//! Single-pass generated-index measurements; run with `cargo run --release
//! -p stella-fuzzy-search-core --example entity_benchmark -- dense`.
//! Modes: dense, random, memory. An optional second argument selects one size.
//! All positions have weight 1 and minimum weight 0.8; forms preserve case and
//! lowercase, and both aliases and queries include every adjacent joined pair.
//! Timings exclude construction, run each query once, and retain its result.
// This command-line benchmark emits CSV measurements for comparison.
#![allow(clippy::print_stdout, clippy::disallowed_macros)]

use std::hint::black_box;
use std::time::Instant;

use stella_fuzzy_search_core::entity::{
  AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
  EntityIndex, EntityIndexBuilder, EntityIndexOptions, EntityQuery, QueryToken,
  QueryUnit, SearchParams,
};

const FORMS: u32 = 2;
const MEMORY_ENTRIES: usize = 40_628;
const MEMORY_ALIASES: usize = 163_140;

fn spelling(text: &str, form: u32) -> String {
  if form == 0 {
    text.to_owned()
  } else {
    text.to_lowercase()
  }
}

fn words(
  mode: &str,
  row: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
  Ok(match mode {
    "dense" => vec![
      "Registered".into(),
      "Entity".into(),
      row.to_string(),
      "Holdings".into(),
    ],
    // Multiplicative permutations give reproducible, unrelated vocabulary
    // combinations without an external random-number dependency.
    "random" => [31_usize, 127, 8191, 0x0007_FFFF]
      .into_iter()
      .enumerate()
      .map(|(position, multiplier)| {
        format!(
          "Word{position}x{:06}",
          row.wrapping_mul(multiplier) % 0xFFF1
        )
      })
      .collect(),
    // Four aliases per entry on average, with a common organization suffix,
    // medium-frequency location and family tokens, and a rare identity token.
    // This specifies 652,560 token positions per form at the default size.
    "memory" => vec![
      ["Company", "Holdings", "Limited", "Group"]
        .get(row % 4)
        .copied()
        .ok_or("Missing benchmark suffix")?
        .into(),
      format!("Region{:03}", row.wrapping_mul(31) % 257),
      format!("Family{:04}", row.wrapping_mul(127) % 4093),
      format!("Identity{:05}", row % MEMORY_ENTRIES),
    ],
    _ => return Err("unknown benchmark mode".into()),
  })
}

fn build(
  mode: &str,
  count: usize,
) -> Result<EntityIndex, Box<dyn std::error::Error>> {
  let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
    forms: FORMS,
    budget: EditBudget {
      by_length: vec![0, 0, 0, 1, 1, 2],
      basis: BudgetBasis::Shorter,
    },
    initial_weight: 1.0,
    initial_similarity: 0.8,
  })?;
  let mut vocabulary_sizes = vec![0; usize::try_from(FORMS)?];
  for row in 0..count {
    let words = words(mode, row)?;
    let mut forms = Vec::new();
    for form in 0..FORMS {
      let mut tokens = Vec::new();
      let mut joins = Vec::new();
      let vocabulary_size = vocabulary_sizes
        .get_mut(usize::try_from(form)?)
        .ok_or("Missing benchmark spelling form")?;
      for word in &words {
        let id = builder.intern(form, &spelling(word, form))?;
        *vocabulary_size = (*vocabulary_size).max(
          usize::try_from(id)?
            .checked_add(1)
            .ok_or("Benchmark vocabulary size overflow")?,
        );
        tokens.push(AliasToken {
          id,
          weight: 1.0,
          minimum_weight: 0.8,
        });
      }
      for (left, pair) in words.windows(2).enumerate() {
        let id = builder.intern(form, &spelling(&pair.concat(), form))?;
        *vocabulary_size = (*vocabulary_size).max(
          usize::try_from(id)?
            .checked_add(1)
            .ok_or("Benchmark vocabulary size overflow")?,
        );
        joins.push(AliasJoin {
          id,
          left: u32::try_from(left)?,
          right: u32::try_from(
            left
              .checked_add(1)
              .ok_or("Benchmark join position overflow")?,
          )?,
        });
      }
      forms.push(AliasForm {
        form,
        minimum_total: 0.8 * f64::from(u32::try_from(tokens.len())?),
        tokens,
        joins,
      });
    }
    builder.add_alias(AliasInput {
      entry_id: u32::try_from(if mode == "memory" {
        row % MEMORY_ENTRIES
      } else {
        row
      })?,
      entity_group: 0,
      quality: 1.0,
      forms,
    })?;
  }
  println!(
    "index,{mode},{count},token_positions_per_form={},vocabulary_per_form={vocabulary_sizes:?}",
    count
      .checked_mul(4)
      .ok_or("Benchmark token count overflow")?
  );
  Ok(
    builder.finish(
      vocabulary_sizes
        .into_iter()
        .map(|size| vec![1.0; size])
        .collect(),
    )?,
  )
}

fn query(words: &[String]) -> Result<EntityQuery, Box<dyn std::error::Error>> {
  let tokens = words
    .iter()
    .map(|text| QueryToken {
      form: 0,
      text: text.clone(),
      weight: 1.0,
    })
    .collect();
  let mut units = Vec::new();
  for form in 0..FORMS {
    for (position, word) in words.iter().enumerate() {
      units.push(QueryUnit {
        form,
        text: spelling(word, form),
        positions: vec![u32::try_from(position)?],
      });
    }
    for (left, pair) in words.windows(2).enumerate() {
      units.push(QueryUnit {
        form,
        text: spelling(&pair.concat(), form),
        positions: vec![
          u32::try_from(left)?,
          u32::try_from(
            left
              .checked_add(1)
              .ok_or("Benchmark join position overflow")?,
          )?,
        ],
      });
    }
  }
  Ok(EntityQuery { tokens, units })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let args = std::env::args().skip(1).collect::<Vec<_>>();
  let mode = args.first().map_or("dense", String::as_str);
  if !["dense", "random", "memory"].contains(&mode) || args.len() > 2 {
    return Err("usage: entity_benchmark [dense|random|memory] [size]".into());
  }
  let sizes = if let Some(size) = args.get(1) {
    vec![size.parse()?]
  } else if mode == "memory" {
    vec![MEMORY_ALIASES]
  } else {
    vec![1250, 10_000, 20_000]
  };
  println!(
    "mode,aliases,query,search_ms,survivors,lexicon_cells,lexicon_matches,sorted_accesses,random_access_pairs,native_bytes,postings"
  );
  for count in sizes {
    let index = build(mode, count)?;
    let queries = if mode == "dense" {
      vec![
        vec![
          "Registered".into(),
          "Entity".into(),
          "42".into(),
          "Holdings".into(),
        ],
        vec!["Registered".into(), "Entity".into(), "Holdings".into()],
      ]
    } else {
      vec![words(mode, 42)?]
    };
    for words in queries {
      let query = query(&words)?;
      let start = Instant::now();
      let result = black_box(index.search(
        black_box(&query),
        &SearchParams {
          tau: 0.9,
          quality: 1.0,
          max_cost: u64::MAX,
        },
      )?);
      let elapsed = start.elapsed();
      if result.exhausted {
        return Err("benchmark exhausted its work budget".into());
      }
      println!(
        "{mode},{count},{},{:.3},{},{},{},{},{},{},{}",
        words.join(" "),
        elapsed.as_secs_f64() * 1000.0,
        result.survivors.len(),
        result.cost.lexicon_cells,
        result.cost.lexicon_matches,
        result.cost.sorted_accesses,
        result.cost.random_access_pairs,
        index.native_bytes(),
        index.posting_count()
      );
    }
  }
  Ok(())
}
