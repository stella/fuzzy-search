//! Stateful construction; all vocabulary and posting work finishes in steps.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::forward;
use super::lexicon::Lexicon;
use super::postings::{PostingBuilder, Postings};
use super::{
  AliasId, AliasInput, EntityIndex, EntityIndexOptions, FormId, TokenId,
};
use crate::{Error, Result};

#[derive(Debug)]
enum BuildState {
  Collecting,
  Lexicons {
    next: usize,
    built: Vec<Arc<Lexicon>>,
  },
  Aliases {
    next: usize,
    lexicons: Vec<Arc<Lexicon>>,
    postings: PostingBuilder,
  },
  Prepared {
    lexicons: Vec<Arc<Lexicon>>,
    postings: Postings,
  },
  Failed,
}

/// Caller-order interning and incremental preparation of immutable index data.
#[derive(Debug)]
pub struct EntityIndexBuilder {
  options: EntityIndexOptions,
  vocabularies: Vec<Vec<String>>,
  ids: Vec<BTreeMap<String, TokenId>>,
  aliases: Vec<AliasInput>,
  state: BuildState,
}

fn invalid(message: &str) -> Error {
  Error::from_reason(message)
}

impl EntityIndexBuilder {
  pub fn new(options: EntityIndexOptions) -> Result<Self> {
    options.budget.validate()?;
    if options.forms == 0
      || !options.initial_weight.is_finite()
      || options.initial_weight <= 0.0
      || !options.initial_similarity.is_finite()
      || !(0.0..=1.0).contains(&options.initial_similarity)
    {
      return Err(invalid("Invalid entity index options"));
    }
    let forms = usize::try_from(options.forms)
      .map_err(|_| invalid("Entity form count exceeds usize"))?;
    let mut vocabularies = Vec::new();
    let mut ids = Vec::new();
    vocabularies
      .try_reserve_exact(forms)
      .map_err(|_| invalid("Entity form allocation failed"))?;
    ids
      .try_reserve_exact(forms)
      .map_err(|_| invalid("Entity form allocation failed"))?;
    for _ in 0..forms {
      vocabularies.push(Vec::new());
      ids.push(BTreeMap::new());
    }
    Ok(Self {
      options,
      vocabularies,
      ids,
      aliases: Vec::new(),
      state: BuildState::Collecting,
    })
  }

  fn collecting(&self) -> Result<()> {
    if !matches!(self.state, BuildState::Collecting) {
      return Err(invalid(
        "Entity builder no longer accepts input after finishing starts",
      ));
    }
    Ok(())
  }

  pub fn intern(&mut self, form: FormId, text: &str) -> Result<TokenId> {
    self.collecting()?;
    if text.is_empty() || u16::try_from(text.chars().count()).is_err() {
      return Err(invalid(
        "Entity tokens must be nonempty with u16 character length",
      ));
    }
    let form = usize::try_from(form)
      .map_err(|_| invalid("Invalid entity spelling form"))?;
    let known = self
      .ids
      .get_mut(form)
      .ok_or_else(|| invalid("Invalid entity spelling form"))?;
    if let Some(id) = known.get(text) {
      return Ok(*id);
    }
    let tokens = self
      .vocabularies
      .get_mut(form)
      .ok_or_else(|| invalid("Invalid entity spelling form"))?;
    let id = u32::try_from(tokens.len())
      .map_err(|_| invalid("Entity vocabulary exceeds u32"))?;
    tokens.push(text.to_owned());
    known.insert(text.to_owned(), id);
    Ok(id)
  }

  pub fn add_alias(&mut self, mut alias: AliasInput) -> Result<AliasId> {
    self.collecting()?;
    forward::validate_alias(&alias, &self.vocabularies)?;
    let id = u32::try_from(self.aliases.len())
      .map_err(|_| invalid("Entity aliases exceed u32"))?;
    alias.forms.shrink_to_fit();
    for form in &mut alias.forms {
      form.tokens.shrink_to_fit();
      form.joins.shrink_to_fit();
    }
    self.aliases.push(alias);
    Ok(id)
  }

  /// One unit builds a vocabulary, projects one alias, or freezes the postings.
  /// Final `finish` only validates and attaches the query-weight tables.
  pub fn finish_step(&mut self, max_units: u32) -> Result<bool> {
    if max_units == 0 {
      return Err(invalid("Entity finish step requires positive max_units"));
    }
    for _ in 0..max_units {
      let state = std::mem::replace(&mut self.state, BuildState::Failed);
      match state {
        BuildState::Failed => {
          return Err(invalid("Entity builder preparation failed"));
        }
        BuildState::Collecting => {
          self.ids.clear();
          self.state = BuildState::Lexicons {
            next: 0,
            built: Vec::new(),
          };
        }
        BuildState::Lexicons { next, mut built } => {
          if let Some(tokens) = self.vocabularies.get(next) {
            let shared = built.iter().find(|lexicon| {
              lexicon.len() == tokens.len()
                && tokens.iter().enumerate().all(|(id, text)| {
                  u32::try_from(id).ok().and_then(|id| lexicon.token(id))
                    == Some(text.as_str())
                })
            });
            let lexicon = if let Some(shared) = shared {
              Arc::clone(shared)
            } else {
              Arc::new(Lexicon::new(tokens)?)
            };
            built.push(lexicon);
            self.state = BuildState::Lexicons {
              next: next.saturating_add(1),
              built,
            };
          } else {
            self.vocabularies.clear();
            self.state = BuildState::Aliases {
              next: 0,
              lexicons: built,
              postings: PostingBuilder::default(),
            };
          }
        }
        BuildState::Aliases {
          next,
          lexicons,
          mut postings,
        } => {
          if let Some(alias) = self.aliases.get(next) {
            let id = u32::try_from(next)
              .map_err(|_| invalid("Entity alias exceeds u32"))?;
            forward::post_alias(
              &mut postings,
              id,
              alias,
              &lexicons,
              &self.options,
            )?;
            self.state = BuildState::Aliases {
              next: next.saturating_add(1),
              lexicons,
              postings,
            };
          } else {
            self.aliases.shrink_to_fit();
            self.state = BuildState::Prepared {
              lexicons,
              postings: postings.finish()?,
            };
          }
        }
        prepared @ BuildState::Prepared { .. } => {
          self.state = prepared;
          return Ok(true);
        }
      }
      if matches!(self.state, BuildState::Prepared { .. }) {
        return Ok(true);
      }
    }
    Ok(false)
  }

  /// Without prior stepping, prepares synchronously before attaching weights.
  pub fn finish(mut self, token_weights: Vec<Vec<f64>>) -> Result<EntityIndex> {
    while !self.finish_step(u32::MAX)? {}
    let BuildState::Prepared { lexicons, postings } = self.state else {
      return Err(invalid("Entity index preparation did not complete"));
    };
    if token_weights.len() != lexicons.len() {
      return Err(invalid("Entity weights must cover every spelling form"));
    }
    for (weights, lexicon) in token_weights.iter().zip(&lexicons) {
      if weights.len() != lexicon.len()
        || weights
          .iter()
          .any(|weight| !weight.is_finite() || *weight <= 0.0)
      {
        return Err(invalid(
          "Entity weights must be finite positive values for every token",
        ));
      }
    }
    Ok(EntityIndex {
      options: self.options,
      lexicons,
      aliases: self.aliases,
      postings,
      token_weights,
    })
  }
}
