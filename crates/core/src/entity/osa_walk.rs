//! Banded Unicode OSA automaton for byte-oriented FST traversal.

use std::cell::Cell;
use std::collections::HashMap;

use fst::Automaton;

const INLINE_CELLS: usize = 8;

#[derive(Clone, Debug)]
enum Cells {
  Inline {
    values: [u16; INLINE_CELLS],
    len: usize,
  },
  Heap(Vec<u16>),
}

impl Cells {
  fn with_capacity(capacity: usize) -> Self {
    if capacity <= INLINE_CELLS {
      Self::Inline {
        values: [0; INLINE_CELLS],
        len: 0,
      }
    } else {
      Self::Heap(Vec::with_capacity(capacity))
    }
  }

  fn push(&mut self, value: usize) {
    let value = u16::try_from(value).unwrap_or(u16::MAX);
    match self {
      Self::Inline { values, len } => {
        if let Some(slot) = values.get_mut(*len) {
          *slot = value;
          *len = len.saturating_add(1);
        } else {
          let mut heap = values.to_vec();
          heap.push(value);
          *self = Self::Heap(heap);
        }
      }
      Self::Heap(values) => values.push(value),
    }
  }

  fn get(&self, index: usize) -> Option<usize> {
    match self {
      Self::Inline { values, len } => {
        values.get(..*len)?.get(index).copied().map(usize::from)
      }
      Self::Heap(values) => values.get(index).copied().map(usize::from),
    }
  }

  fn last(&self) -> Option<usize> {
    match self {
      Self::Inline { values, len } => len
        .checked_sub(1)
        .and_then(|index| values.get(index))
        .copied()
        .map(usize::from),
      Self::Heap(values) => values.last().copied().map(usize::from),
    }
  }

  fn iter(&self) -> impl Iterator<Item = usize> + '_ {
    let slice = match self {
      // SAFETY: `push` transitions to heap storage instead of incrementing
      // `len` beyond the inline array.
      #[allow(clippy::indexing_slicing)]
      Self::Inline { values, len } => &values[..*len],
      Self::Heap(values) => values,
    };
    slice.iter().copied().map(usize::from)
  }

  fn initial(length: usize) -> Self {
    let mut cells = Self::with_capacity(length.saturating_add(1));
    for value in 0..=length {
      cells.push(value);
    }
    cells
  }
}

#[derive(Clone, Debug)]
struct Row {
  start: usize,
  cells: Cells,
}

impl Row {
  fn get(&self, column: usize, infinity: usize) -> usize {
    column
      .checked_sub(self.start)
      .and_then(|index| self.cells.get(index))
      .unwrap_or(infinity)
  }
}

#[derive(Clone, Debug)]
pub(super) struct State {
  row: Option<Box<Row>>,
  previous_row: Option<Box<Row>>,
  previous_char: Option<char>,
  pub(super) length: usize,
  utf8: [u8; 4],
  filled: usize,
  expected: usize,
  live: bool,
  bits: Option<BitState>,
}

#[derive(Clone, Copy, Debug)]
struct BitState {
  positive: u64,
  negative: u64,
  previous_matches: u64,
  previous_zeroes: u64,
  distance: usize,
}

struct CharacterMasks {
  ascii: [u64; 128],
  unicode: HashMap<char, u64>,
}

impl State {
  pub(super) fn distance(&self, query_length: usize, infinity: usize) -> usize {
    self.bits.map_or_else(
      || {
        self
          .row
          .as_deref()
          .map_or(infinity, |row| row.get(query_length, infinity))
      },
      |bits| bits.distance,
    )
  }
}

pub(super) struct OsaAutomaton {
  query: Vec<char>,
  budget: usize,
  cells: Cell<u64>,
  limit: u64,
  matches: Cell<u64>,
  exhausted: Cell<bool>,
  masks: Option<CharacterMasks>,
}

impl OsaAutomaton {
  pub(super) fn new(query: &str, budget: u8, limit: u64) -> Self {
    let query = query.chars().collect::<Vec<_>>();
    let masks = (!query.is_empty() && query.len() <= 64).then(|| {
      let mut masks = CharacterMasks {
        ascii: [0; 128],
        unicode: HashMap::new(),
      };
      for (column, character) in query.iter().copied().enumerate() {
        if character.is_ascii() {
          if let Some(mask) = masks
            .ascii
            .get_mut(usize::try_from(u32::from(character)).unwrap_or(0))
          {
            *mask |= 1_u64 << column;
          }
        } else {
          masks
            .unicode
            .entry(character)
            .and_modify(|mask| *mask |= 1_u64 << column)
            .or_insert(1_u64 << column);
        }
      }
      masks
    });
    Self {
      query,
      budget: usize::from(budget),
      cells: Cell::new(0),
      limit,
      matches: Cell::new(0),
      exhausted: Cell::new(false),
      masks,
    }
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
    if let (Some(masks), Some(bits)) = (&self.masks, state.bits) {
      return self.advance_bits(state, bits, masks, character);
    }
    self.advance_row(state, character)
  }

  /// Hyyrö's bit-parallel OSA recurrence. Work is still charged as if the
  /// diagonal row cells were visited, preserving the public deterministic cost.
  #[allow(clippy::arithmetic_side_effects)]
  fn advance_bits(
    &self,
    state: &State,
    bits: BitState,
    masks: &CharacterMasks,
    character: char,
  ) -> State {
    let length = state.length + 1;
    let start = length.saturating_sub(self.budget);
    let end = (length + self.budget).min(self.query.len());
    if start > end {
      return Self::dead_state(length);
    }
    let charged = end.saturating_sub(start.max(1)).saturating_add(1);
    let available = self
      .limit
      .saturating_sub(self.cells.get().saturating_add(self.matches.get()));
    if u64::try_from(charged).unwrap_or(u64::MAX) > available {
      self.cells.set(self.cells.get().saturating_add(available));
      self.exhausted.set(true);
      return Self::dead_state(length);
    }
    self.cells.set(
      self
        .cells
        .get()
        .saturating_add(u64::try_from(charged).unwrap_or(u64::MAX)),
    );

    let matches = if character.is_ascii() {
      masks
        .ascii
        .get(usize::try_from(u32::from(character)).unwrap_or(0))
        .copied()
        .unwrap_or(0)
    } else {
      masks.unicode.get(&character).copied().unwrap_or(0)
    };
    let x = matches | bits.negative;
    let mut zeroes =
      (((x & bits.positive).wrapping_add(bits.positive)) ^ bits.positive) | x;
    // A zero two rows ago followed by crossed character matches represents
    // one adjacent transposition rather than two substitutions.
    zeroes |= ((!bits.previous_zeroes & matches) << 1) & bits.previous_matches;
    let positive_horizontal = bits.negative | !(zeroes | bits.positive);
    let negative_horizontal = bits.positive & zeroes;
    let top = 1_u64 << self.query.len().saturating_sub(1);
    let distance = bits
      .distance
      .saturating_add(usize::from(positive_horizontal & top != 0))
      .saturating_sub(usize::from(negative_horizontal & top != 0));
    let shifted = (positive_horizontal << 1) | 1;
    let negative = shifted & zeroes;
    let positive = (negative_horizontal << 1) | !(shifted | zeroes);

    // Recover the minimum DP cell from the horizontal difference vectors.
    let query_bits = if self.query.len() == 64 {
      u64::MAX
    } else {
      (1_u64 << self.query.len()) - 1
    };
    let after_end = if end == 64 {
      0
    } else {
      query_bits & (u64::MAX << end)
    };
    let mut cell = distance
      .saturating_sub(
        usize::try_from((positive & after_end).count_ones())
          .unwrap_or(usize::MAX),
      )
      .saturating_add(
        usize::try_from((negative & after_end).count_ones())
          .unwrap_or(usize::MAX),
      );
    let mut minimum = cell;
    for column in (start..end).rev() {
      let bit = 1_u64 << column;
      cell = cell
        .saturating_sub(usize::from(positive & bit != 0))
        .saturating_add(usize::from(negative & bit != 0));
      minimum = minimum.min(cell);
    }
    State {
      row: None,
      previous_row: None,
      previous_char: Some(character),
      length,
      utf8: [0; 4],
      filled: 0,
      expected: 0,
      live: minimum <= self.budget,
      bits: Some(BitState {
        positive,
        negative,
        previous_matches: matches,
        previous_zeroes: zeroes,
        distance,
      }),
    }
  }

  #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
  fn advance_row(&self, state: &State, character: char) -> State {
    let length = state.length + 1;
    let start = length.saturating_sub(self.budget);
    let end = (length + self.budget).min(self.query.len());
    let infinity = self.budget + 1;
    if start > end {
      return Self::dead_state(length);
    }
    let mut cells = Cells::with_capacity(end - start + 1);
    for column in start..=end {
      if column == 0 {
        cells.push(length.min(infinity));
        continue;
      }
      if self.cells.get().saturating_add(self.matches.get()) >= self.limit {
        self.exhausted.set(true);
        return Self::dead_state(length);
      }
      self.cells.set(self.cells.get() + 1);
      let deletion = state
        .row
        .as_deref()
        .map_or(infinity, |row| row.get(column, infinity))
        + 1;
      let insertion = cells.last().unwrap_or(infinity) + 1;
      let substitution = state
        .row
        .as_deref()
        .map_or(infinity, |row| row.get(column - 1, infinity))
        + usize::from(self.query[column - 1] != character);
      let mut value = deletion.min(insertion).min(substitution);
      if column > 1
        && state.previous_char == Some(self.query[column - 1])
        && character == self.query[column - 2]
      {
        value = value.min(
          state
            .previous_row
            .as_deref()
            .map_or(infinity, |row| row.get(column - 2, infinity))
            + 1,
        );
      }
      cells.push(value.min(infinity));
    }
    let live = cells.iter().any(|value| value <= self.budget);
    State {
      row: Some(Box::new(Row { start, cells })),
      previous_row: state.row.clone(),
      previous_char: Some(character),
      length,
      utf8: [0; 4],
      filled: 0,
      expected: 0,
      live,
      bits: None,
    }
  }

  const fn dead_state(length: usize) -> State {
    State {
      row: None,
      previous_row: None,
      previous_char: None,
      length,
      utf8: [0; 4],
      filled: 0,
      expected: 0,
      live: false,
      bits: None,
    }
  }
}

impl Automaton for OsaAutomaton {
  type State = State;

  fn start(&self) -> State {
    State {
      row: self.masks.is_none().then(|| {
        Box::new(Row {
          start: 0,
          cells: Cells::initial(self.query.len().min(self.budget)),
        })
      }),
      previous_row: None,
      previous_char: None,
      length: 0,
      utf8: [0; 4],
      filled: 0,
      expected: 0,
      live: true,
      bits: self.masks.as_ref().map(|_| BitState {
        positive: u64::MAX,
        negative: 0,
        previous_matches: 0,
        previous_zeroes: 0,
        distance: self.query.len(),
      }),
    }
  }

  fn is_match(&self, state: &State) -> bool {
    state.live
      && state.filled == 0
      && state.bits.map_or_else(
        || state.distance(self.query.len(), self.budget.saturating_add(1)),
        |bits| bits.distance,
      ) <= self.budget
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
