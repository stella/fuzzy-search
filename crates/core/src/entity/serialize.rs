//! Versioned source-data encoding. Derived lexicons and postings are rebuilt.

use super::{
  AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
  EntityIndex, EntityIndexBuilder, EntityIndexOptions,
};
use crate::{Error, Result};

const MAGIC: &[u8; 8] = b"STLLEIDX";
const MIN_SUPPORTED_VERSION: u32 = 1;
const VERSION: u32 = 2;

fn invalid(reason: &str) -> Error {
  Error::from_reason(format!("Invalid entity index encoding: {reason}"))
}

struct Writer(Vec<u8>);

impl Writer {
  fn u32(&mut self, value: u32) {
    self.0.extend_from_slice(&value.to_le_bytes());
  }

  fn count(&mut self, value: usize) -> Result<()> {
    self.u32(u32::try_from(value).map_err(|_| invalid("count exceeds u32"))?);
    Ok(())
  }

  fn bytes(&mut self, bytes: &[u8]) -> Result<()> {
    self.count(bytes.len())?;
    self.0.extend_from_slice(bytes);
    Ok(())
  }

  fn f64(&mut self, value: f64) -> Result<()> {
    if !value.is_finite() {
      return Err(invalid("non-finite weight"));
    }
    self.0.extend_from_slice(&value.to_le_bytes());
    Ok(())
  }
}

struct Reader<'a> {
  remaining: &'a [u8],
}

impl<'a> Reader<'a> {
  fn take(&mut self, length: usize) -> Result<&'a [u8]> {
    let (head, tail) = self
      .remaining
      .split_at_checked(length)
      .ok_or_else(|| invalid("truncated payload"))?;
    self.remaining = tail;
    Ok(head)
  }

  fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
    self
      .take(N)?
      .try_into()
      .map_err(|_| invalid("invalid scalar width"))
  }

  fn u32(&mut self) -> Result<u32> {
    Ok(u32::from_le_bytes(self.array()?))
  }

  fn bounded_count(&self, count: u32, minimum_bytes: usize) -> Result<usize> {
    let count =
      usize::try_from(count).map_err(|_| invalid("count overflow"))?;
    if count
      .checked_mul(minimum_bytes)
      .is_none_or(|bytes| bytes > self.remaining.len())
    {
      return Err(invalid("count exceeds remaining payload"));
    }
    Ok(count)
  }

  fn count(&mut self, minimum_bytes: usize) -> Result<usize> {
    let count = self.u32()?;
    self.bounded_count(count, minimum_bytes)
  }

  fn bytes(&mut self) -> Result<&'a [u8]> {
    let length = self.count(1)?;
    self.take(length)
  }

  fn f64(&mut self) -> Result<f64> {
    let value = f64::from_le_bytes(self.array()?);
    if !value.is_finite() {
      return Err(invalid("non-finite weight"));
    }
    Ok(value)
  }
}

pub(super) fn to_bytes(index: &EntityIndex) -> Result<Vec<u8>> {
  let mut writer = Writer(Vec::new());
  writer.0.extend_from_slice(MAGIC);
  writer.u32(VERSION);
  let options = &index.options;
  writer.u32(options.forms);
  match options.budget.basis {
    BudgetBasis::Shorter => writer.0.push(0),
  }
  writer.bytes(&options.budget.by_length)?;
  writer.f64(options.initial_weight)?;
  writer.f64(options.initial_similarity)?;
  let forms = usize::try_from(options.forms)
    .map_err(|_| invalid("form count overflow"))?;
  if index.lexicons.len() != forms || index.token_weights.len() != forms {
    return Err(invalid("form count mismatch"));
  }
  for (lexicon, weights) in index.lexicons.iter().zip(&index.token_weights) {
    if lexicon.len() != weights.len() {
      return Err(invalid("vocabulary weight count mismatch"));
    }
    writer.count(lexicon.len())?;
    for (id, weight) in weights.iter().enumerate() {
      let id = u32::try_from(id).map_err(|_| invalid("token id overflow"))?;
      let token = lexicon.token(id).ok_or_else(|| invalid("missing token"))?;
      writer.bytes(token.as_bytes())?;
      writer.f64(*weight)?;
    }
  }
  writer.count(index.aliases.len())?;
  for alias in index.aliases.iter() {
    writer.u32(alias.entry_id);
    writer.u32(alias.entity_group);
    writer.f64(alias.quality)?;
    writer.count(alias.forms.len())?;
    for form in &alias.forms {
      writer.u32(form.form);
      writer.f64(form.minimum_total)?;
      writer.count(form.tokens.len())?;
      for token in &form.tokens {
        writer.u32(token.id);
        writer.f64(token.weight)?;
        writer.f64(token.minimum_weight)?;
      }
      writer.count(form.joins.len())?;
      for join in &form.joins {
        writer.u32(join.id);
        writer.u32(join.left);
        writer.u32(join.right);
      }
    }
  }
  Ok(writer.0)
}

fn read_options(reader: &mut Reader<'_>) -> Result<EntityIndexOptions> {
  let forms = reader.u32()?;
  let basis = match reader.array::<1>()? {
    [0] => BudgetBasis::Shorter,
    _ => return Err(invalid("unknown budget basis")),
  };
  let by_length = reader.bytes()?.to_vec();
  let initial_weight = reader.f64()?;
  let initial_similarity = reader.f64()?;
  // Every form still needs at least its vocabulary count, even if empty.
  reader.bounded_count(forms, 4)?;
  Ok(EntityIndexOptions {
    forms,
    budget: EditBudget { by_length, basis },
    initial_weight,
    initial_similarity,
  })
}

fn read_form(reader: &mut Reader<'_>) -> Result<AliasForm> {
  let form = reader.u32()?;
  let minimum_total = reader.f64()?;
  let token_count = reader.count(20)?;
  let mut tokens = Vec::with_capacity(token_count);
  for _ in 0..token_count {
    tokens.push(AliasToken {
      id: reader.u32()?,
      weight: reader.f64()?,
      minimum_weight: reader.f64()?,
    });
  }
  let join_count = reader.count(12)?;
  let mut joins = Vec::with_capacity(join_count);
  for _ in 0..join_count {
    joins.push(AliasJoin {
      id: reader.u32()?,
      left: reader.u32()?,
      right: reader.u32()?,
    });
  }
  Ok(AliasForm {
    form,
    tokens,
    joins,
    minimum_total,
  })
}

fn read_alias(reader: &mut Reader<'_>) -> Result<AliasInput> {
  let entry_id = reader.u32()?;
  let entity_group = reader.u32()?;
  let quality = reader.f64()?;
  let form_count = reader.count(20)?;
  let mut forms = Vec::with_capacity(form_count);
  for _ in 0..form_count {
    forms.push(read_form(reader)?);
  }
  Ok(AliasInput {
    entry_id,
    entity_group,
    quality,
    forms,
  })
}

pub(super) fn from_bytes(bytes: &[u8]) -> Result<EntityIndex> {
  let mut reader = Reader { remaining: bytes };
  if reader.take(MAGIC.len())? != MAGIC {
    return Err(invalid("unknown magic"));
  }
  let version = reader.u32()?;
  if !(MIN_SUPPORTED_VERSION..=VERSION).contains(&version) {
    return Err(Error {
      reason: format!(
        "Unsupported entity index version {version}; expected {VERSION}"
      ),
      kind: crate::ErrorKind::EntityIndexUnsupportedVersion,
    });
  }
  let options = read_options(&mut reader)?;
  let form_count = options.forms;
  let mut builder = EntityIndexBuilder::new(options)?;
  let mut token_weights = Vec::new();
  for form in 0..form_count {
    let count = reader.count(12)?;
    let mut weights = Vec::with_capacity(count);
    for id in 0..count {
      let text = std::str::from_utf8(reader.bytes()?)
        .map_err(|_| invalid("invalid UTF-8 token"))?;
      let weight = reader.f64()?;
      let actual = builder.intern(form, text)?;
      let expected =
        u32::try_from(id).map_err(|_| invalid("token id overflow"))?;
      if actual != expected {
        return Err(invalid("duplicate vocabulary token"));
      }
      weights.push(weight);
    }
    token_weights.push(weights);
  }
  let alias_count = reader.count(20)?;
  for _ in 0..alias_count {
    builder.add_alias(read_alias(&mut reader)?)?;
  }
  if !reader.remaining.is_empty() {
    return Err(invalid("trailing bytes"));
  }
  builder.finish(token_weights)
}

#[cfg(test)]
mod tests {
  use proptest::prelude::*;

  use super::super::{EntityQuery, QueryToken, QueryUnit, SearchParams};
  use super::{
    AliasForm, AliasInput, AliasJoin, AliasToken, BudgetBasis, EditBudget,
    EntityIndex, EntityIndexBuilder, EntityIndexOptions, MAGIC,
    MIN_SUPPORTED_VERSION, Reader, Result, VERSION, Writer, from_bytes,
    invalid, read_options, to_bytes,
  };

  fn fixture() -> Result<EntityIndex> {
    let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
      forms: 2,
      budget: EditBudget {
        by_length: vec![0, 0, 0, 0, 1, 1, 2],
        basis: BudgetBasis::Shorter,
      },
      initial_weight: 1.0,
      initial_similarity: 0.8,
    })?;
    let mut forms = Vec::new();
    for form in 0..2 {
      for text in ["álpha", "東京", "álpha東京"] {
        builder.intern(form, text)?;
      }
      forms.push(AliasForm {
        form,
        tokens: vec![
          AliasToken {
            id: 0,
            weight: 2.0,
            minimum_weight: 2.0,
          },
          AliasToken {
            id: 1,
            weight: 3.0,
            minimum_weight: 3.0,
          },
        ],
        joins: vec![AliasJoin {
          id: 2,
          left: 0,
          right: 1,
        }],
        minimum_total: 5.0,
      });
    }
    builder.add_alias(AliasInput {
      entry_id: 7,
      entity_group: 9,
      quality: 0.9,
      forms,
    })?;
    builder.finish(vec![vec![2.0, 3.0, 4.0], vec![2.0, 3.0, 4.0]])
  }

  fn replace(
    bytes: &mut [u8],
    offset: usize,
    replacement: &[u8],
  ) -> Result<()> {
    let end = offset
      .checked_add(replacement.len())
      .ok_or_else(|| invalid("test offset overflow"))?;
    bytes
      .get_mut(offset..end)
      .ok_or_else(|| invalid("test offset out of bounds"))?
      .copy_from_slice(replacement);
    Ok(())
  }

  // Enumerate all length/count fields from an independently encoded source
  // fixture, including fields nested inside aliases. This pins the wire format.
  fn encoded_fixture() -> Result<(Vec<u8>, Vec<usize>)> {
    let mut writer = Writer(Vec::new());
    let mut counts = Vec::new();
    writer.0.extend_from_slice(MAGIC);
    writer.u32(VERSION);
    counts.push(writer.0.len());
    writer.u32(2);
    writer.0.push(0);
    counts.push(writer.0.len());
    writer.bytes(&[0, 0, 0, 0, 1, 1, 2])?;
    writer.f64(1.0)?;
    writer.f64(0.8)?;
    for _ in 0..2 {
      counts.push(writer.0.len());
      writer.count(3)?;
      for (text, weight) in [("álpha", 2.0), ("東京", 3.0), ("álpha東京", 4.0)]
      {
        counts.push(writer.0.len());
        writer.bytes(text.as_bytes())?;
        writer.f64(weight)?;
      }
    }
    counts.push(writer.0.len());
    writer.count(1)?;
    writer.u32(7);
    writer.u32(9);
    writer.f64(0.9)?;
    counts.push(writer.0.len());
    writer.count(2)?;
    for form in 0..2 {
      writer.u32(form);
      writer.f64(5.0)?;
      counts.push(writer.0.len());
      writer.count(2)?;
      for (id, weight) in [(0, 2.0), (1, 3.0)] {
        writer.u32(id);
        writer.f64(weight)?;
        writer.f64(weight)?;
      }
      counts.push(writer.0.len());
      writer.count(1)?;
      writer.u32(2);
      writer.u32(0);
      writer.u32(1);
    }
    Ok((writer.0, counts))
  }

  #[test]
  fn source_data_roundtrip_is_byte_stable() -> Result<()> {
    let index = fixture()?;
    let bytes = to_bytes(&index)?;
    let (expected, _) = encoded_fixture()?;
    assert_eq!(
      bytes, expected,
      "source encoding must match the versioned layout"
    );
    let restored = from_bytes(&bytes)?;
    assert_eq!(restored.options, index.options, "options must roundtrip");
    assert_eq!(
      restored.aliases, index.aliases,
      "full alias data must roundtrip"
    );
    assert_eq!(
      restored.token_weights, index.token_weights,
      "vocabulary weights must roundtrip"
    );
    for (left, right) in restored.lexicons.iter().zip(&index.lexicons) {
      assert_eq!(left.len(), right.len(), "vocabulary sizes must roundtrip");
      for id in 0..u32::try_from(left.len())
        .map_err(|_| invalid("test vocabulary too large"))?
      {
        assert_eq!(
          left.token(id),
          right.token(id),
          "caller token ids must roundtrip"
        );
      }
    }
    assert_eq!(
      to_bytes(&restored)?,
      bytes,
      "rebuilding must preserve bytes"
    );
    Ok(())
  }

  #[test]
  fn rejects_every_truncated_prefix() -> Result<()> {
    let bytes = to_bytes(&fixture()?)?;
    for end in 0..bytes.len() {
      let prefix = bytes
        .get(..end)
        .ok_or_else(|| invalid("test prefix out of bounds"))?;
      assert!(
        from_bytes(prefix).is_err(),
        "truncated prefix {end} must fail"
      );
    }
    Ok(())
  }

  #[test]
  fn rejects_oversized_counts_at_every_nested_boundary() -> Result<()> {
    let (bytes, offsets) = encoded_fixture()?;
    for offset in offsets {
      let mut malformed = bytes.clone();
      replace(&mut malformed, offset, &u32::MAX.to_le_bytes())?;
      assert!(
        from_bytes(&malformed).is_err(),
        "oversized count at {offset} must fail before allocation"
      );
    }
    Ok(())
  }

  #[test]
  fn rejects_unknown_format_utf8_and_trailing_data() -> Result<()> {
    let bytes = to_bytes(&fixture()?)?;
    let mut malformed = bytes.clone();
    replace(&mut malformed, 0, b"?")?;
    assert!(from_bytes(&malformed).is_err(), "unknown magic must fail");
    malformed.clone_from(&bytes);
    replace(
      &mut malformed,
      MAGIC.len(),
      &VERSION.saturating_add(1).to_le_bytes(),
    )?;
    assert!(from_bytes(&malformed).is_err(), "unknown version must fail");
    malformed.clone_from(&bytes);
    let mut reader = Reader { remaining: &bytes };
    reader.take(MAGIC.len())?;
    reader.u32()?;
    read_options(&mut reader)?;
    reader.u32()?;
    reader.u32()?;
    let token_offset = bytes.len().saturating_sub(reader.remaining.len());
    replace(&mut malformed, token_offset, &[0xff])?;
    assert!(from_bytes(&malformed).is_err(), "invalid UTF-8 must fail");
    malformed.clone_from(&bytes);
    malformed.push(0);
    assert!(from_bytes(&malformed).is_err(), "trailing bytes must fail");
    Ok(())
  }

  #[test]
  fn rejects_nonfinite_numbers() -> Result<()> {
    let bytes = to_bytes(&fixture()?)?;
    let mut reader = Reader { remaining: &bytes };
    reader.take(MAGIC.len())?;
    reader.u32()?;
    reader.u32()?;
    reader.take(1)?;
    reader.bytes()?;
    let weight_offset = bytes.len().saturating_sub(reader.remaining.len());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
      let mut malformed = bytes.clone();
      replace(&mut malformed, weight_offset, &value.to_le_bytes())?;
      assert!(
        from_bytes(&malformed).is_err(),
        "non-finite options must fail"
      );
    }
    Ok(())
  }

  #[test]
  #[allow(clippy::expect_used)]
  fn rejects_all_other_versions_with_typed_error() -> Result<()> {
    let bytes = to_bytes(&fixture()?)?;
    for version in [0_u32, VERSION.saturating_add(1), u32::MAX] {
      let mut malformed = bytes.clone();
      replace(&mut malformed, MAGIC.len(), &version.to_le_bytes())?;
      let error =
        from_bytes(&malformed).expect_err("unsupported versions must fail");
      assert_eq!(
        error.kind(),
        crate::ErrorKind::EntityIndexUnsupportedVersion
      );
      assert!(error.reason().contains(&version.to_string()));
    }
    Ok(())
  }

  #[test]
  fn reads_version_one_payload_with_unchanged_search_results() -> Result<()> {
    let index = fixture()?;
    let mut bytes = to_bytes(&index)?;
    replace(
      &mut bytes,
      MAGIC.len(),
      &MIN_SUPPORTED_VERSION.to_le_bytes(),
    )?;
    let restored = from_bytes(&bytes)?;
    let query = EntityQuery {
      tokens: vec![
        QueryToken {
          form: 0,
          text: "álpha".to_owned(),
          weight: 2.0,
        },
        QueryToken {
          form: 0,
          text: "東京".to_owned(),
          weight: 3.0,
        },
      ],
      units: vec![
        QueryUnit {
          form: 0,
          text: "álpha".to_owned(),
          positions: vec![0],
        },
        QueryUnit {
          form: 0,
          text: "東京".to_owned(),
          positions: vec![1],
        },
        QueryUnit {
          form: 0,
          text: "álpha東京".to_owned(),
          positions: vec![0, 1],
        },
      ],
    };
    let params = SearchParams {
      tau: 0.0,
      quality: 1.0,
      max_cost: u64::MAX,
    };
    assert_eq!(
      restored.search(&query, &params)?,
      index.search(&query, &params)?
    );
    Ok(())
  }

  #[test]
  fn roundtrip_preserves_different_minima_for_equal_form_weights() -> Result<()>
  {
    let source = fixture()?;
    let mut original = source.aliases.iter().collect::<Vec<_>>();
    for alias in &mut original {
      for form in &mut alias.forms {
        let divisor = f64::from(form.form.saturating_add(2));
        for token in &mut form.tokens {
          token.minimum_weight = token.weight / divisor;
        }
        form.minimum_total =
          form.tokens.iter().map(|token| token.minimum_weight).sum();
      }
    }
    let mut builder = EntityIndexBuilder::new(source.options)?;
    for form in 0..2 {
      for text in ["álpha", "東京", "álpha東京"] {
        builder.intern(form, text)?;
      }
    }
    for input in &original {
      builder.add_alias(input.clone())?;
    }
    let index = builder.finish(vec![vec![2.0, 3.0, 4.0]; 2])?;
    let restored = from_bytes(&to_bytes(&index)?)?;
    assert_eq!(index.aliases.iter().collect::<Vec<_>>(), original);
    assert_eq!(restored.aliases.iter().collect::<Vec<_>>(), original);
    Ok(())
  }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn source_roundtrip_preserves_search_and_encoding(
      rows in prop::collection::vec((prop::collection::vec((0_u32..3, 1_u16..1000, any::<u8>()), 2..6), 0_u16..=1000), 1..10),
      form_count in 1_u32..4,
      tau in 0_u16..=1000,
      quality in 0_u16..=1000,
    ) {
      let mut builder = EntityIndexBuilder::new(EntityIndexOptions {
        forms: form_count,
        budget: EditBudget { by_length: vec![0, 0, 0, 1, 2], basis: BudgetBasis::Shorter },
        initial_weight: 0.25,
        initial_similarity: 0.8,
      }).unwrap();
      let spellings = (0..form_count).map(|form| {
        ["álpha", "東京", "a", "álphaálpha", "álpha東京"].map(|text| if form % 2 == 0 { text.to_owned() } else { text.to_uppercase() })
      }).collect::<Vec<_>>();
      for (form, words) in spellings.iter().enumerate() {
        for word in words { builder.intern(u32::try_from(form).unwrap(), word).unwrap(); }
      }
      let mut original = Vec::new();
      for (entry_id, (positions, alias_quality)) in rows.into_iter().enumerate() {
        let tokens = positions.iter().map(|&(id, weight, discount)| AliasToken {
          id,
          weight: f64::from(weight),
          minimum_weight: f64::from(weight) * (f64::from(discount) + 1.0) / 256.0,
        }).collect::<Vec<_>>();
        let last = u32::try_from(tokens.len().checked_sub(1).unwrap()).unwrap();
        let forms = (0..form_count).map(|form| {
          let form_tokens = tokens.iter().map(|token| AliasToken {
            id: token.id,
            weight: token.weight,
            minimum_weight: token.minimum_weight / f64::from(form.checked_add(1).unwrap()),
          }).collect::<Vec<_>>();
          AliasForm {
            form,
            minimum_total: form_tokens.iter().map(|token| token.minimum_weight).sum(),
            tokens: form_tokens,
            joins: vec![AliasJoin { id: 3, left: 0, right: 0 }, AliasJoin { id: 4, left: last, right: 0 }],
          }
        }).collect();
        let input = AliasInput {
          entry_id: u32::try_from(entry_id).unwrap(),
          entity_group: u32::try_from(entry_id % 3).unwrap(),
          quality: f64::from(alias_quality) / 1000.0,
          forms,
        };
        builder.add_alias(input.clone()).unwrap();
        original.push(input);
      }
      let index = builder.finish(vec![vec![1.0, 2.0, 0.25, 3.0, 4.0]; usize::try_from(form_count).unwrap()]).unwrap();
      let bytes = to_bytes(&index).unwrap();
      let restored = from_bytes(&bytes).unwrap();
      prop_assert_eq!(index.aliases.iter().collect::<Vec<_>>(), original.clone());
      prop_assert_eq!(restored.aliases.iter().collect::<Vec<_>>(), original);
      prop_assert_eq!(to_bytes(&restored).unwrap(), bytes);
      let tokens = ["álpha", "東京"].into_iter().map(|text| QueryToken { form: 0, text: text.to_owned(), weight: 1.0 }).collect();
      let mut units = Vec::new();
      for (form, words) in spellings.iter().enumerate() {
        for position in 0..2 {
          units.push(QueryUnit { form: u32::try_from(form).unwrap(), text: words.get(position).unwrap().clone(), positions: vec![u32::try_from(position).unwrap()] });
        }
        units.push(QueryUnit { form: u32::try_from(form).unwrap(), text: words.get(4).unwrap().clone(), positions: vec![0, 1] });
      }
      let query = EntityQuery { tokens, units };
      for cutoff in [0.0, f64::from(tau) / 1000.0, 1.0] {
        let params = SearchParams { tau: cutoff, quality: f64::from(quality) / 1000.0, max_cost: u64::MAX };
        prop_assert_eq!(index.search(&query, &params).unwrap(), restored.search(&query, &params).unwrap());
      }
    }
  }
}
