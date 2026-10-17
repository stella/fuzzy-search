//! Banded Unicode OSA automaton for byte-oriented FST traversal.

use std::cell::{Cell, RefCell};

use fst::Automaton;

#[derive(Clone, Debug)]
struct Row {
  start: usize,
  cells: Vec<usize>,
}

impl Row {
  fn get(&self, column: usize, infinity: usize) -> usize {
    column
      .checked_sub(self.start)
      .and_then(|index| self.cells.get(index))
      .copied()
      .unwrap_or(infinity)
  }
}

#[derive(Clone, Debug)]
pub(super) struct State {
  row: Row,
  previous_row: Row,
  previous_char: Option<char>,
  pub(super) length: usize,
  utf8: [u8; 4],
  filled: usize,
  expected: usize,
  live: bool,
}

impl State {
  pub(super) fn distance(&self, query_length: usize, infinity: usize) -> usize {
    self.row.get(query_length, infinity)
  }
}

pub(super) struct OsaAutomaton {
  query: Vec<char>,
  budget: usize,
  cells: Cell<u64>,
  limit: u64,
  matches: Cell<u64>,
  exhausted: Cell<bool>,
  prefix: RefCell<Vec<(char, State)>>,
}

impl OsaAutomaton {
  pub(super) fn new(query: &str, budget: u8, limit: u64) -> Self {
    Self {
      query: query.chars().collect(),
      budget: usize::from(budget),
      cells: Cell::new(0),
      limit,
      matches: Cell::new(0),
      exhausted: Cell::new(false),
      prefix: RefCell::new(Vec::new()),
    }
  }

  /// Evaluate surviving candidates in lexical order, sharing their prefix rows.
  /// Skipped tokens need no states; the cache is bounded by one candidate's length.
  pub(super) fn distance_for(&self, text: &str) -> Option<usize> {
    let start = self.start();
    let mut prefix = self.prefix.borrow_mut();
    let mut length = 0_usize;
    for (index, character) in text.chars().enumerate() {
      length = index.saturating_add(1);
      if prefix
        .get(index)
        .is_some_and(|(cached, _)| *cached == character)
      {
        continue;
      }
      prefix.truncate(index);
      let state = prefix.last().map_or(&start, |(_, state)| state);
      if !self.can_match(state) {
        return None;
      }
      let next = self.advance(state, character);
      prefix.push((character, next));
    }
    prefix.truncate(length);
    let state = prefix.last().map_or(&start, |(_, state)| state);
    self
      .is_match(state)
      .then(|| state.distance(self.query.len(), self.budget.saturating_add(1)))
  }

  pub(super) const fn cells(&self) -> u64 {
    self.cells.get()
  }

  pub(super) const fn exhausted(&self) -> bool {
    self.exhausted.get()
  }

  pub(super) fn charge_match(&self) -> bool {
    if self.cells.get().saturating_add(self.matches.get()) >= self.limit {
      self.exhausted.set(true);
      return false;
    }
    self.matches.set(self.matches.get().saturating_add(1));
    true
  }

  pub(super) const fn matches(&self) -> u64 {
    self.matches.get()
  }

  /// Complete one Unicode scalar; only the diagonal band is materialized.
  #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
  fn advance(&self, state: &State, character: char) -> State {
    let length = state.length + 1;
    let start = length.saturating_sub(self.budget);
    let end = (length + self.budget).min(self.query.len());
    let infinity = self.budget + 1;
    let mut next = state.clone();
    next.filled = 0;
    next.expected = 0;
    next.length = length;
    if start > end {
      next.live = false;
      return next;
    }
    let mut cells = Vec::with_capacity(end - start + 1);
    for column in start..=end {
      if column == 0 {
        cells.push(length.min(infinity));
        continue;
      }
      if self.cells.get().saturating_add(self.matches.get()) >= self.limit {
        self.exhausted.set(true);
        next.live = false;
        return next;
      }
      self.cells.set(self.cells.get() + 1);
      let deletion = state.row.get(column, infinity) + 1;
      let insertion = cells.last().copied().unwrap_or(infinity) + 1;
      let substitution = state.row.get(column - 1, infinity)
        + usize::from(self.query[column - 1] != character);
      let mut value = deletion.min(insertion).min(substitution);
      if column > 1
        && state.previous_char == Some(self.query[column - 1])
        && character == self.query[column - 2]
      {
        value = value.min(state.previous_row.get(column - 2, infinity) + 1);
      }
      cells.push(value.min(infinity));
    }
    next.live = cells.iter().any(|&value| value <= self.budget);
    next.row = Row { start, cells };
    next.previous_row = state.row.clone();
    next.previous_char = Some(character);
    next
  }
}

impl Automaton for OsaAutomaton {
  type State = State;

  fn start(&self) -> State {
    State {
      row: Row {
        start: 0,
        cells: (0..=self.query.len().min(self.budget)).collect(),
      },
      previous_row: Row {
        start: 0,
        cells: Vec::new(),
      },
      previous_char: None,
      length: 0,
      utf8: [0; 4],
      filled: 0,
      expected: 0,
      live: true,
    }
  }

  fn is_match(&self, state: &State) -> bool {
    state.live
      && state.filled == 0
      && state.distance(self.query.len(), self.budget.saturating_add(1))
        <= self.budget
  }

  fn can_match(&self, state: &State) -> bool {
    state.live && !self.exhausted.get()
  }

  #[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
  fn accept(&self, state: &State, byte: u8) -> State {
    if !self.can_match(state) {
      let mut dead = state.clone();
      dead.live = false;
      return dead;
    }
    if state.filled == 0 && byte.is_ascii() {
      return self.advance(state, char::from(byte));
    }
    let mut next = state.clone();
    if next.filled == 0 {
      next.expected = match byte {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => {
          next.live = false;
          return next;
        }
      };
    } else if !(0x80..=0xBF).contains(&byte) {
      next.live = false;
      return next;
    }
    next.utf8[next.filled] = byte;
    next.filled += 1;
    if next.filled < next.expected {
      return next;
    }
    let character = std::str::from_utf8(&next.utf8[..next.filled])
      .ok()
      .and_then(|text| text.chars().next());
    let Some(character) = character else {
      next.live = false;
      return next;
    };
    self.advance(&next, character)
  }
}
