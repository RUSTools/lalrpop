//! Core LR(1) types.

use collections::Map;
use grammar::repr::*;
use itertools::Itertools;
use std::fmt::{Debug, Formatter, Error};
use std::rc::Rc;
use util::Prefix;

use super::lookahead::Token;

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item<'grammar, L: Clone> {
    pub production: &'grammar Production,
    /// the dot comes before `index`, so `index` would be 1 for X = A (*) B C
    pub index: usize,
    pub lookahead: L,
}

pub type LR0Item<'grammar> = Item<'grammar, ()>;

pub type LR1Item<'grammar> = Item<'grammar, Token>;

impl<'grammar> Item<'grammar, ()> {
    #[cfg(test)]
    pub fn lr0(production: &'grammar Production,
               index: usize)
               -> Self {
        Item { production: production, index: index, lookahead: () }
    }
}

impl<'grammar, L: Clone> Item<'grammar, L> {
    pub fn prefix(&self) -> &'grammar [Symbol] {
        &self.production.symbols[..self.index]
    }

    pub fn symbol_sets(&self) -> SymbolSets<'grammar> {
        let symbols = &self.production.symbols;
        if self.can_shift() {
            SymbolSets {
                prefix: &symbols[..self.index],
                cursor: Some(&symbols[self.index]),
                suffix: &symbols[self.index+1..],
            }
        } else {
            SymbolSets {
                prefix: &symbols[..self.index],
                cursor: None,
                suffix: &[],
            }
        }
    }

    pub fn to_lr0(&self) -> LR0Item<'grammar> {
        Item { production: self.production, index: self.index, lookahead: () }
    }

    pub fn can_shift(&self) -> bool {
        self.index < self.production.symbols.len()
    }

    pub fn can_reduce(&self) -> bool {
        self.index == self.production.symbols.len()
    }

    pub fn shifted_item(&self) -> Option<(Symbol, Item<'grammar, L>)> {
        if self.can_shift() {
            Some((self.production.symbols[self.index],
                  Item { production: self.production,
                         index: self.index + 1,
                         lookahead: self.lookahead.clone() }))
        } else {
            None
        }
    }

    pub fn shift_symbol(&self) -> Option<(Symbol, &[Symbol])> {
        if self.can_shift() {
            Some((self.production.symbols[self.index], &self.production.symbols[self.index+1..]))
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateIndex(pub usize);

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Items<'grammar, L: Clone> {
    pub vec: Rc<Vec<Item<'grammar, L>>>
}

pub type LR0Items<'grammar> = Items<'grammar, ()>;
pub type LR1Items<'grammar> = Items<'grammar, Token>;

#[derive(Debug)]
pub struct State<'grammar, L: Clone> {
    pub index: StateIndex,
    pub items: Items<'grammar, L>,
    pub tokens: Map<Token, Action<'grammar>>,
    pub conflicts: Map<Token, Vec<Conflict<'grammar>>>,
    pub gotos: Map<NonterminalString, StateIndex>,
}

pub type LR0State<'grammar> = State<'grammar, ()>;
pub type LR1State<'grammar> = State<'grammar, Token>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action<'grammar> {
    Shift(StateIndex),
    Reduce(&'grammar Production),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Conflict<'grammar> {
    // when in this state...
    pub state: StateIndex,

    // we can reduce...
    pub production: &'grammar Production,

    // but we can also...
    pub action: Action<'grammar>,
}

#[derive(Debug)]
pub struct TableConstructionError<'grammar> {
    // LR(1) state set. Some of these states are in error.
    pub states: Vec<LR1State<'grammar>>,
}

impl<'grammar, L: Clone+Debug> Debug for Item<'grammar, L> {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        let mut lookahead_debug = format!("{:?}", self.lookahead);
        if lookahead_debug != "()" {
            lookahead_debug = format!(" [{}]", lookahead_debug);
        } else {
            lookahead_debug = format!("");
        }

        write!(fmt, "{} ={} (*){}{}",
               self.production.nonterminal,
               Prefix(" ", &self.production.symbols[..self.index]),
               Prefix(" ", &self.production.symbols[self.index..]),
               lookahead_debug)
    }
}

impl Debug for Token {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        match *self {
            Token::EOF => write!(fmt, "EOF"),
            Token::Terminal(s) => write!(fmt, "{}", s),
        }
    }
}

impl Debug for StateIndex {
    fn fmt(&self, fmt: &mut Formatter) -> Result<(), Error> {
        write!(fmt, "S{}", self.0)
    }
}

impl<'grammar, L: Clone> State<'grammar, L> {
    /// Returns the set of symbols which must appear on the stack to
    /// be in this state. This is the *maximum* prefix of any item,
    /// basically.
    pub fn max_prefix(&self) -> &'grammar [Symbol] {
        // Each state fn takes as argument the longest prefix of any
        // item. Note that all items must have compatible prefixes.
        let prefix =
            self.items.vec
                      .iter()
                      .map(|item| item.prefix())
                      .max_by_key(|symbols| symbols.len())
                      .unwrap();

        debug_assert!(
            self.items.vec
                      .iter()
                      .all(|item| prefix.ends_with(&item.production.symbols[..item.index])));

        prefix
    }

    /// Returns the set of symbols from the stack that must be popped
    /// for this state to return. If we have a state like:
    ///
    /// ```
    /// X = A B C (*) C
    /// Y = B C (*) C
    /// C = (*) ...
    /// ```
    ///
    /// This would return `[B, C]`. For every state other than the
    /// start state, this will return a list of length at least 1.
    /// For the start state, returns `[]`.
    pub fn will_pop(&self) -> &'grammar [Symbol] {
        let prefix =
            self.items.vec.iter()
                          .filter(|item| item.index > 0)
                          .map(|item| item.prefix())
                          .min_by_key(|symbols| symbols.len())
                          .unwrap_or(&[]);

        debug_assert!(
            self.items.vec
                      .iter()
                      .filter(|item| item.index > 0)
                      .all(|item| item.prefix().ends_with(prefix)));

        prefix
    }

    pub fn will_push(&self) -> &[Symbol] {
        self.items.vec.iter()
                      .filter(|item| item.index > 0)
                      .map(|item| &item.production.symbols[item.index..])
                      .min_by_key(|symbols| symbols.len())
                      .unwrap_or(&[])
    }

    /// Returns the type of nonterminal that this state will produce;
    /// if `None` is returned, then this state may produce more than
    /// one kind of nonterminal.
    ///
    /// FIXME -- currently, the start state returns `None` instead of
    /// the goal symbol.
    pub fn will_produce(&self) -> Option<NonterminalString> {
        let mut returnable_nonterminals: Vec<_> =
            self.items.vec.iter()
                          .filter(|item| item.index > 0)
                          .map(|item| item.production.nonterminal)
                          .dedup()
                          .collect();
        if returnable_nonterminals.len() == 1 {
            returnable_nonterminals.pop()
        } else {
            None
        }
    }

}

impl<'grammar> Action<'grammar> {
    pub fn shift(&self) -> Option<StateIndex> {
        match *self {
            Action::Shift(next_index) => Some(next_index),
            _ => None
        }
    }
    pub fn reduce(&self) -> Option<&'grammar Production> {
        match *self {
            Action::Reduce(production) => Some(production),
            _ => None,
        }
    }
}

/// `A = B C (*) D E F` or `A = B C (*)`
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SymbolSets<'grammar> {
    pub prefix: &'grammar [Symbol], // both cases, [B, C]
    pub cursor: Option<&'grammar Symbol>, // first [D], second []
    pub suffix: &'grammar [Symbol], // first [E, F], second []
}

impl<'grammar> SymbolSets<'grammar> {
    pub fn new() -> Self {
        SymbolSets {
            prefix: &[],
            cursor: None,
            suffix: &[],
        }
    }
}
