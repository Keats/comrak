mod table;
mod autolink;
mod inlines;

use arena_tree::Node;
use ctype::{isspace, isdigit};
use entity;
use nodes;
use nodes::{NodeValue, Ast, NodeCodeBlock, NodeHeading, NodeList, ListType, ListDelimType,
            NodeHtmlBlock, make_block, AstNode};
use regex::bytes::Regex;
use scanners;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::min;
use std::collections::HashMap;
use std::mem;
use std::ops::Index;
use std::ptr;
use std::str;
use strings;
use typed_arena::Arena;

const TAB_STOP: usize = 4;
const CODE_INDENT: usize = 4;

/// Parse a Markdown document to an AST.
///
/// See the documentation of the crate root for an example.
pub fn parse_document<'a>(
    arena: &'a Arena<AstNode<'a>>,
    buffer: &'a str,
    options: &ComrakOptions,
) -> &'a AstNode<'a> {
    let root: &'a AstNode<'a> = arena.alloc(Node::new(RefCell::new(Ast {
        value: NodeValue::Document,
        content: Cow::from(vec![]),
        start_line: 0,
        start_column: 0,
        end_line: 0,
        end_column: 0,
        open: true,
        last_line_blank: false,
    })));
    let mut parser = Parser::new(arena, root, options);
    parser.feed(buffer.as_bytes(), true);
    parser.finish()
}

pub struct Parser<'a, 'o> {
    arena: &'a Arena<AstNode<'a>>,
    refmap: HashMap<Vec<u8>, Reference<'a>>,
    root: &'a AstNode<'a>,
    current: &'a AstNode<'a>,
    line_number: u32,
    offset: usize,
    column: usize,
    first_nonspace: usize,
    first_nonspace_column: usize,
    indent: usize,
    blank: bool,
    partially_consumed_tab: bool,
    last_line_length: usize,
    linebuf: Vec<u8>,
    last_buffer_ended_with_cr: bool,
    options: &'o ComrakOptions,
}

#[derive(Default, Debug, Clone, Copy)]
/// Options for both parser and formatter functions.
pub struct ComrakOptions {
    /// [Soft line breaks](http://spec.commonmark.org/0.27/#soft-line-breaks) in the input
    /// translate into hard line breaks in the output.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// assert_eq!(markdown_to_html("Hello.\nWorld.\n", &options),
    ///            "<p>Hello.\nWorld.</p>\n");
    ///
    /// options.hardbreaks = true;
    /// assert_eq!(markdown_to_html("Hello.\nWorld.\n", &options),
    ///            "<p>Hello.<br />\nWorld.</p>\n");
    /// ```
    pub hardbreaks: bool,

    /// GitHub-style `<pre lang="xyz">` is used for fenced code blocks with info tags.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// assert_eq!(markdown_to_html("``` rust\nfn hello();\n```\n", &options),
    ///            "<pre><code class=\"language-rust\">fn hello();\n</code></pre>\n");
    ///
    /// options.github_pre_lang = true;
    /// assert_eq!(markdown_to_html("``` rust\nfn hello();\n```\n", &options),
    ///            "<pre lang=\"rust\"><code>fn hello();\n</code></pre>\n");
    /// ```
    pub github_pre_lang: bool,

    /// The wrap column when outputting CommonMark.
    ///
    /// ```
    /// # extern crate typed_arena;
    /// # extern crate comrak;
    /// # use comrak::{parse_document, ComrakOptions, format_commonmark};
    /// # fn main() {
    /// # let arena = typed_arena::Arena::new();
    /// let mut options = ComrakOptions::default();
    /// let node = parse_document(&arena, "hello hello hello hello hello hello", &options);
    /// let mut output = vec![];
    /// format_commonmark(node, &options, &mut output).unwrap();
    /// assert_eq!(String::from_utf8(output).unwrap(),
    ///            "hello hello hello hello hello hello\n");
    ///
    /// options.width = 20;
    /// let mut output = vec![];
    /// format_commonmark(node, &options, &mut output).unwrap();
    /// assert_eq!(String::from_utf8(output).unwrap(),
    ///            "hello hello hello\nhello hello hello\n");
    /// # }
    /// ```
    pub width: usize,

    /// Enables the
    /// [striket,hrough extension](https://github.github.com/gfm/#strikethrough-extension-)
    /// from the GFM spec.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_strikethrough = true;
    /// assert_eq!(markdown_to_html("Hello ~world~ there.\n", &options),
    ///            "<p>Hello <del>world</del> there.</p>\n");
    /// ```
    pub ext_strikethrough: bool,

    /// Enables the
    /// [tagfilter extension](https://github.github.com/gfm/#disallowed-raw-html-extension-)
    /// from the GFM spec.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_tagfilter = true;
    /// assert_eq!(markdown_to_html("Hello <xmp>.\n\n<xmp>", &options),
    ///            "<p>Hello &lt;xmp>.</p>\n&lt;xmp>\n");
    /// ```
    pub ext_tagfilter: bool,

    /// Enables the [table extension](https://github.github.com/gfm/#tables-extension-)
    /// from the GFM spec.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_table = true;
    /// assert_eq!(markdown_to_html("| a | b |\n|---|---|\n| c | d |\n", &options),
    ///            "<table>\n<thead>\n<tr>\n<th>a</th>\n<th>b</th>\n</tr>\n</thead>\n\
    ///             <tbody>\n<tr>\n<td>c</td>\n<td>d</td>\n</tr></tbody></table>\n");
    /// ```
    pub ext_table: bool,

    /// Enables the [autolink extension](https://github.github.com/gfm/#autolinks-extension-)
    /// from the GFM spec.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_autolink = true;
    /// assert_eq!(markdown_to_html("Hello www.github.com.\n", &options),
    ///            "<p>Hello <a href=\"http://www.github.com\">www.github.com</a>.</p>\n");
    /// ```
    pub ext_autolink: bool,

    /// Enables the
    /// [task list items extension](https://github.github.com/gfm/#task-list-items-extension-)
    /// from the GFM spec.
    ///
    /// Note that the spec does not define the precise output, so only the bare essentials are
    /// rendered.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_tasklist = true;
    /// assert_eq!(markdown_to_html("* [x] Done\n* [ ] Not done\n", &options),
    ///            "<ul>\n<li><input type=\"checkbox\" disabled=\"\" checked=\"\" /> Done</li>\n\
    ///            <li><input type=\"checkbox\" disabled=\"\" /> Not done</li>\n</ul>\n");
    /// ```
    pub ext_tasklist: bool,

    /// Enables the superscript Comrak extension.
    ///
    /// ```
    /// # use comrak::{markdown_to_html, ComrakOptions};
    /// let mut options = ComrakOptions::default();
    /// options.ext_superscript = true;
    /// assert_eq!(markdown_to_html("e = mc^2^.\n", &options),
    ///            "<p>e = mc<sup>2</sup>.</p>\n");
    /// ```
    pub ext_superscript: bool,
}


#[derive(Clone)]
pub struct Reference<'a> {
    pub url: Cow<'a, [u8]>,
    pub title: Cow<'a, [u8]>,
}

impl<'a, 'o> Parser<'a, 'o> {
    pub fn new(
        arena: &'a Arena<AstNode<'a>>,
        root: &'a AstNode<'a>,
        options: &'o ComrakOptions,
    ) -> Parser<'a, 'o> {
        Parser {
            arena: arena,
            refmap: HashMap::new(),
            root: root,
            current: root,
            line_number: 0,
            offset: 0,
            column: 0,
            first_nonspace: 0,
            first_nonspace_column: 0,
            indent: 0,
            blank: false,
            partially_consumed_tab: false,
            last_line_length: 0,
            linebuf: Vec::with_capacity(80),
            last_buffer_ended_with_cr: false,
            options: options,
        }
    }

    pub fn feed(&mut self, s: &'a [u8], eof: bool) {
        let mut i = 0;
        let buffer = s;
        let sz = buffer.len();

        if self.last_buffer_ended_with_cr && buffer[i] == b'\n' {
            i += 1;
        }
        self.last_buffer_ended_with_cr = false;

        while i < sz {
            let mut process = false;
            let mut eol = i;
            while eol < sz {
                if strings::is_line_end_char(buffer[eol]) {
                    process = true;
                    break;
                }
                if buffer[eol] == 0 {
                    break;
                }
                eol += 1;
            }

            if eol >= sz && eof {
                process = true;
            }

            if process {
                if !self.linebuf.is_empty() {
                    self.linebuf.extend_from_slice(&s[i..eol]);
                    let linebuf = mem::replace(&mut self.linebuf, Vec::with_capacity(80));
                    self.process_line(linebuf.into());
                } else if sz > eol && buffer[eol] == b'\n' {
                    self.process_line((&s[i..eol + 1]).into());
                } else {
                    self.process_line((&s[i..eol]).into());
                }
            } else if eol < sz && buffer[eol] == b'\0' {
                self.linebuf.extend_from_slice(&s[i..eol]);
                self.linebuf.extend_from_slice(&"\u{fffd}".to_string().into_bytes());
                eol += 1;
            } else {
                self.linebuf.extend_from_slice(&s[i..eol]);
            }

            i = eol;
            if i < sz && buffer[i] == b'\r' {
                i += 1;
                if i == sz {
                    self.last_buffer_ended_with_cr = true;
                }
            }
            if i < sz && buffer[i] == b'\n' {
                i += 1;
            }
        }
    }

    fn find_first_nonspace(&mut self, line: &[u8]) {
        self.first_nonspace = self.offset;
        self.first_nonspace_column = self.column;
        let mut chars_to_tab = TAB_STOP - (self.column % TAB_STOP);

        loop {
            if self.first_nonspace >= line.len() {
                break;
            }
            match line[self.first_nonspace] {
                32 => {
                    self.first_nonspace += 1;
                    self.first_nonspace_column += 1;
                    chars_to_tab -= 1;
                    if chars_to_tab == 0 {
                        chars_to_tab = TAB_STOP;
                    }
                }
                9 => {
                    self.first_nonspace += 1;
                    self.first_nonspace_column += chars_to_tab;
                    chars_to_tab = TAB_STOP;
                }
                _ => break,
            }
        }

        self.indent = self.first_nonspace_column - self.column;
        self.blank = self.first_nonspace < line.len() &&
            strings::is_line_end_char(line[self.first_nonspace]);
    }

    fn process_line(&mut self, mut line: Cow<'a, [u8]>) {
        if line.is_empty() || !strings::is_line_end_char(*line.last().unwrap()) {
            line.to_mut().push(b'\n');
        }

        self.offset = 0;
        self.column = 0;
        self.blank = false;
        self.partially_consumed_tab = false;

        if self.line_number == 0 && line.len() >= 3 && unsafe { str::from_utf8_unchecked(&line) }.chars().next().unwrap() == '\u{feff}' {
            self.offset += 3;
        }

        self.line_number += 1;

        let mut all_matched = true;
        if let Some(last_matched_container) = self.check_open_blocks(&line, &mut all_matched) {
            let mut container = last_matched_container;
            let current = self.current;
            self.open_new_blocks(&mut container, &line, all_matched);

            if current.same_node(self.current) {
                self.add_text_to_container(container, last_matched_container, &line);
            }
        }

        self.last_line_length = line.len();
        if self.last_line_length > 0 && line[self.last_line_length - 1] == b'\n' {
            self.last_line_length -= 1;
        }
        if self.last_line_length > 0 && line[self.last_line_length - 1] == b'\r' {
            self.last_line_length -= 1;
        }
    }

    fn check_open_blocks(&mut self, line: &[u8], all_matched: &mut bool) -> Option<&'a AstNode<'a>> {
        let (new_all_matched, mut container, should_continue) =
            self.check_open_blocks_inner(self.root, line);

        *all_matched = new_all_matched;
        if !*all_matched {
            container = container.parent().unwrap();
        }

        if !should_continue {
            None
        } else {
            Some(container)
        }
    }

    fn check_open_blocks_inner(
        &mut self,
        mut container: &'a AstNode<'a>,
        line: &[u8],
    ) -> (bool, &'a AstNode<'a>, bool) {
        let mut should_continue = true;

        while nodes::last_child_is_open(container) {
            container = container.last_child().unwrap();
            let ast = &mut *container.data.borrow_mut();

            self.find_first_nonspace(line);

            match ast.value {
                NodeValue::BlockQuote => {
                    if !self.parse_block_quote_prefix(line) {
                        return (false, container, should_continue);
                    }
                }
                NodeValue::Item(ref nl) => {
                    if !self.parse_node_item_prefix(line, container, nl) {
                        return (false, container, should_continue);
                    }
                }
                NodeValue::CodeBlock(..) => {
                    if !self.parse_code_block_prefix(line, container, ast, &mut should_continue) {
                        return (false, container, should_continue);
                    }
                }
                NodeValue::HtmlBlock(ref nhb) => {
                    if !self.parse_html_block_prefix(nhb.block_type) {
                        return (false, container, should_continue);
                    }
                }
                NodeValue::Paragraph => {
                    if self.blank {
                        return (false, container, should_continue);
                    }
                }
                NodeValue::Table(..) => {
                    if !table::matches(&line[self.first_nonspace..]) {
                        return (false, container, should_continue);
                    }
                    continue;
                }
                NodeValue::Heading(..) |
                NodeValue::TableRow(..) |
                NodeValue::TableCell => {
                    return (false, container, should_continue);
                }
                _ => {}
            }
        }

        (true, container, should_continue)
    }

    fn open_new_blocks(&mut self, container: &mut &'a AstNode<'a>, line: &Cow<'a, [u8]>, all_matched: bool) {
        let mut matched: usize = 0;
        let mut nl: NodeList = NodeList::default();
        let mut sc: scanners::SetextChar = scanners::SetextChar::Equals;
        let mut maybe_lazy = match self.current.data.borrow().value {
            NodeValue::Paragraph => true,
            _ => false,
        };

        while match container.data.borrow().value {
            NodeValue::CodeBlock(..) |
            NodeValue::HtmlBlock(..) => false,
            _ => true,
        }
        {
            self.find_first_nonspace(line);
            let indented = self.indent >= CODE_INDENT;

            if !indented && line[self.first_nonspace] == b'>' {
                let blockquote_startpos = self.first_nonspace;
                let offset = self.first_nonspace + 1 - self.offset;
                self.advance_offset(line, offset, false);
                if strings::is_space_or_tab(line[self.offset]) {
                    self.advance_offset(line, 1, true);
                }
                *container =
                    self.add_child(*container, NodeValue::BlockQuote, blockquote_startpos + 1);
            } else if !indented &&
                       unwrap_into(
                    scanners::atx_heading_start(&line[self.first_nonspace..]),
                    &mut matched,
                )
            {
                let heading_startpos = self.first_nonspace;
                let offset = self.offset;
                self.advance_offset(line, heading_startpos + matched - offset, false);
                *container = self.add_child(
                    *container,
                    NodeValue::Heading(NodeHeading::default()),
                    heading_startpos + 1,
                );

                let mut hashpos = line[self.first_nonspace..]
                    .iter()
                    .position(|&c| c == b'#')
                    .unwrap() + self.first_nonspace;
                let mut level = 0;
                while line[hashpos] == b'#' {
                    level += 1;
                    hashpos += 1;
                }

                container.data.borrow_mut().value = NodeValue::Heading(NodeHeading {
                    level: level,
                    setext: false,
                });

            } else if !indented &&
                       unwrap_into(
                    scanners::open_code_fence(&line[self.first_nonspace..]),
                    &mut matched,
                )
            {
                let first_nonspace = self.first_nonspace;
                let offset = self.offset;
                let ncb = NodeCodeBlock {
                    fenced: true,
                    fence_char: line[first_nonspace],
                    fence_length: matched,
                    fence_offset: first_nonspace - offset,
                    info: Cow::from(vec![]),
                    literal: Cow::from(vec![]),
                };
                *container =
                    self.add_child(*container, NodeValue::CodeBlock(ncb), first_nonspace + 1);
                self.advance_offset(line, first_nonspace + matched - offset, false);
            } else if !indented &&
                       (unwrap_into(
                    scanners::html_block_start(&line[self.first_nonspace..]),
                    &mut matched,
                ) ||
                            match container.data.borrow().value {
                                NodeValue::Paragraph => false,
                                _ => {
                                    unwrap_into(
                        scanners::html_block_start_7(&line[self.first_nonspace..]),
                        &mut matched,
                    )
                                }
                            })
            {
                let offset = self.first_nonspace + 1;
                let nhb = NodeHtmlBlock {
                    block_type: matched as u8,
                    literal: Cow::from(vec![]),
                };

                *container = self.add_child(*container, NodeValue::HtmlBlock(nhb), offset);
            } else if !indented &&
                       match container.data.borrow().value {
                           NodeValue::Paragraph => {
                               unwrap_into(
                    scanners::setext_heading_line(&line[self.first_nonspace..]),
                    &mut sc,
                )
                           }
                           _ => false,
                       }
            {
                container.data.borrow_mut().value = NodeValue::Heading(NodeHeading {
                    level: match sc {
                        scanners::SetextChar::Equals => 1,
                        scanners::SetextChar::Hyphen => 2,
                    },
                    setext: true,
                });
                let adv = line.len() - 1 - self.offset;
                self.advance_offset(line, adv, false);
            } else if !indented &&
                       match (&container.data.borrow().value, all_matched) {
                           (&NodeValue::Paragraph, false) => false,
                           _ => {
                               unwrap_into(
                    scanners::thematic_break(&line[self.first_nonspace..]),
                    &mut matched,
                )
                           }
                       }
            {
                let offset = self.first_nonspace + 1;
                *container = self.add_child(*container, NodeValue::ThematicBreak, offset);
                let adv = line.len() - 1 - self.offset;
                self.advance_offset(line, adv, false);
            } else if (!indented ||
                           match container.data.borrow().value {
                               NodeValue::List(..) => true,
                               _ => false,
                           }) &&
                       unwrap_into_2(
                    parse_list_marker(
                        line,
                        self.first_nonspace,
                        match container.data.borrow().value {
                            NodeValue::Paragraph => true,
                            _ => false,
                        },
                    ),
                    &mut matched,
                    &mut nl,
                )
            {
                let offset = self.first_nonspace + matched - self.offset;
                self.advance_offset(line, offset, false);
                let (save_partially_consumed_tab, save_offset, save_column) =
                    (self.partially_consumed_tab, self.offset, self.column);

                while self.column - save_column <= 5 &&
                    strings::is_space_or_tab(line[self.offset])
                {
                    self.advance_offset(line, 1, true);
                }

                let i = self.column - save_column;
                if i >= 5 || i < 1 || strings::is_line_end_char(line[self.offset]) {
                    nl.padding = matched + 1;
                    self.offset = save_offset;
                    self.column = save_column;
                    self.partially_consumed_tab = save_partially_consumed_tab;
                    if i > 0 {
                        self.advance_offset(line, 1, true);
                    }
                } else {
                    nl.padding = matched + i;
                }

                nl.marker_offset = self.indent;

                let offset = self.first_nonspace + 1;
                if match container.data.borrow().value {
                    NodeValue::List(ref mnl) => !lists_match(&nl, mnl),
                    _ => true,
                }
                {
                    *container = self.add_child(*container, NodeValue::List(nl), offset);
                }

                let offset = self.first_nonspace + 1;
                *container = self.add_child(*container, NodeValue::Item(nl), offset);
            } else if indented && !maybe_lazy && !self.blank {
                self.advance_offset(line, CODE_INDENT, true);
                let ncb = NodeCodeBlock {
                    fenced: false,
                    fence_char: 0,
                    fence_length: 0,
                    fence_offset: 0,
                    info: Cow::from(vec![]),
                    literal: Cow::from(vec![]),
                };
                let offset = self.offset + 1;
                *container = self.add_child(*container, NodeValue::CodeBlock(ncb), offset);
            } else {
                let new_container = if !indented && self.options.ext_table {
                    table::try_opening_block(self, *container, line)
                } else {
                    None
                };

                match new_container {
                    Some((new_container, replace)) => {
                        if replace {
                            container.insert_after(new_container);
                            container.detach();
                            *container = new_container;
                        } else {
                            *container = new_container;
                        }
                    }
                    _ => break,
                }
            }

            if container.data.borrow().value.accepts_lines() {
                break;
            }

            maybe_lazy = false;
        }
    }

    fn advance_offset(&mut self, line: &[u8], mut count: usize, columns: bool) {
        while count > 0 {
            match line[self.offset] {
                9 => {
                    let chars_to_tab = TAB_STOP - (self.column % TAB_STOP);
                    if columns {
                        self.partially_consumed_tab = chars_to_tab > count;
                        let chars_to_advance = min(count, chars_to_tab);
                        self.column += chars_to_advance;
                        self.offset += if self.partially_consumed_tab { 0 } else { 1 };
                        count -= chars_to_advance;
                    } else {
                        self.partially_consumed_tab = false;
                        self.column += chars_to_tab;
                        self.offset += 1;
                        count -= 1;
                    }
                }
                _ => {
                    self.partially_consumed_tab = false;
                    self.offset += 1;
                    self.column += 1;
                    count -= 1;
                }
            }
        }
    }

    fn parse_block_quote_prefix(&mut self, line: &[u8]) -> bool {
        let indent = self.indent;
        if indent <= 3 && line[self.first_nonspace] == b'>' {
            self.advance_offset(line, indent + 1, true);

            if strings::is_space_or_tab(line[self.offset]) {
                self.advance_offset(line, 1, true);
            }

            return true;
        }

        false
    }

    fn parse_node_item_prefix(
        &mut self,
        line: &[u8],
        container: &'a AstNode<'a>,
        nl: &NodeList,
    ) -> bool {
        if self.indent >= nl.marker_offset + nl.padding {
            self.advance_offset(line, nl.marker_offset + nl.padding, true);
            true
        } else if self.blank && container.first_child().is_some() {
            let offset = self.first_nonspace - self.offset;
            self.advance_offset(line, offset, false);
            true
        } else {
            false
        }
    }

    fn parse_code_block_prefix(
        &mut self,
        line: &[u8],
        container: &'a AstNode<'a>,
        ast: &mut Ast<'a>,
        should_continue: &mut bool,
    ) -> bool {
        let (fenced, fence_char, fence_length, fence_offset) = match ast.value {
            NodeValue::CodeBlock(ref ncb) => {
                (
                    ncb.fenced,
                    ncb.fence_char,
                    ncb.fence_length,
                    ncb.fence_offset,
                )
            }
            _ => unreachable!(),
        };

        if !fenced {
            if self.indent >= CODE_INDENT {
                self.advance_offset(line, CODE_INDENT, true);
                return true;
            } else if self.blank {
                let offset = self.first_nonspace - self.offset;
                self.advance_offset(line, offset, false);
                return true;
            }
            return false;
        }

        let matched = if self.indent <= 3 && line[self.first_nonspace] == fence_char {
            scanners::close_code_fence(&line[self.first_nonspace..]).unwrap_or(0)
        } else {
            0
        };

        if matched >= fence_length {
            *should_continue = false;
            self.advance_offset(line, matched, false);
            self.current = self.finalize_borrowed(container, ast).unwrap();
            return false;

        }

        let mut i = fence_offset;
        while i > 0 && strings::is_space_or_tab(line[self.offset]) {
            self.advance_offset(line, 1, true);
            i -= 1;
        }
        true
    }

    fn parse_html_block_prefix(&mut self, t: u8) -> bool {
        match t {
            1 | 2 | 3 | 4 | 5 => true,
            6 | 7 => !self.blank,
            _ => {
                assert!(false);
                false
            }
        }
    }

    fn add_child(
        &mut self,
        mut parent: &'a AstNode<'a>,
        value: NodeValue<'a>,
        start_column: usize,
    ) -> &'a AstNode<'a> {
        while !nodes::can_contain_type(parent, &value) {
            parent = self.finalize(parent).unwrap();
        }

        let child = make_block(value, self.line_number, start_column);
        let node = self.arena.alloc(Node::new(RefCell::new(child)));
        parent.append(node);
        node
    }

    fn add_text_to_container(
        &mut self,
        mut container: &'a AstNode<'a>,
        last_matched_container: &'a AstNode<'a>,
        line: &Cow<'a, [u8]>,
    ) {
        self.find_first_nonspace(line);

        if self.blank {
            if let Some(last_child) = container.last_child() {
                last_child.data.borrow_mut().last_line_blank = true;
            }
        }

        container.data.borrow_mut().last_line_blank = self.blank &&
            match container.data.borrow().value {
                NodeValue::BlockQuote |
                NodeValue::Heading(..) |
                NodeValue::ThematicBreak => false,
                NodeValue::CodeBlock(ref ncb) => !ncb.fenced,
                NodeValue::Item(..) => {
                    container.first_child().is_some() ||
                        container.data.borrow().start_line != self.line_number
                }
                _ => true,
            };

        let mut tmp = container;
        while let Some(parent) = tmp.parent() {
            parent.data.borrow_mut().last_line_blank = false;
            tmp = parent;
        }

        if !self.current.same_node(last_matched_container) &&
            container.same_node(last_matched_container) && !self.blank &&
            match self.current.data.borrow().value {
                NodeValue::Paragraph => true,
                _ => false,
            }
        {
            self.add_line(self.current, line);
        } else {
            while !self.current.same_node(last_matched_container) {
                self.current = self.finalize(self.current).unwrap();
            }

            let add_text_result = match container.data.borrow().value {
                NodeValue::CodeBlock(..) => AddTextResult::CodeBlock,
                NodeValue::HtmlBlock(ref nhb) => AddTextResult::HtmlBlock(nhb.block_type),
                _ => AddTextResult::Otherwise,
            };

            match add_text_result {
                AddTextResult::CodeBlock => {
                    self.add_line(container, line);
                }
                AddTextResult::HtmlBlock(block_type) => {
                    self.add_line(container, line);

                    let matches_end_condition = match block_type {
                        1 => scanners::html_block_end_1(&line[self.first_nonspace..]),
                        2 => scanners::html_block_end_2(&line[self.first_nonspace..]),
                        3 => scanners::html_block_end_3(&line[self.first_nonspace..]),
                        4 => scanners::html_block_end_4(&line[self.first_nonspace..]),
                        5 => scanners::html_block_end_5(&line[self.first_nonspace..]),
                        _ => false,
                    };

                    if matches_end_condition {
                        container = self.finalize(container).unwrap();
                    }
                }
                _ => {
                    if self.blank {
                        // do nothing
                    } else if container.data.borrow().value.accepts_lines() {
                        let mut line: Cow<'a, [u8]> = line.clone();
                        if let NodeValue::Heading(ref nh) = container.data.borrow().value {
                            if !nh.setext {
                                strings::chop_trailing_hashtags(&mut line);
                            }
                        };
                        let count = self.first_nonspace - self.offset;
                        self.advance_offset(&line, count, false);
                        self.add_line(container, &line);
                    } else {
                        let start_column = self.first_nonspace + 1;
                        container = self.add_child(container, NodeValue::Paragraph, start_column);
                        let count = self.first_nonspace - self.offset;
                        self.advance_offset(line, count, false);
                        self.add_line(container, line);
                    }
                }
            }

            self.current = container;
        }
    }

    fn add_line(&mut self, node: &'a AstNode<'a>, line: &Cow<'a, [u8]>) {
        // TODO: this entire thing needs to be smarter???
        // TODO: this is always copying and we wanna avoid that.

        let mut ast = node.data.borrow_mut();
        assert!(ast.open);
        if self.partially_consumed_tab {
            self.offset += 1;
            let chars_to_tab = TAB_STOP - (self.column % TAB_STOP);
            for _ in 0..chars_to_tab {
                ast.content.to_mut().push(b' ');
            }
        }
        if self.offset < line.len() {
            if ast.content.len() == 0 {
                ast.content = cow_range(line, self.offset..);
            } else {
                cow_append(&mut ast.content, &line[self.offset..]);
            }
        }
    }

    pub fn finish(&mut self) -> &'a AstNode<'a> {
        if !self.linebuf.is_empty() {
            let linebuf = mem::replace(&mut self.linebuf, vec![]);
            self.process_line(linebuf.into());
        }

        self.finalize_document();
        self.postprocess_text_nodes(self.root);
        self.root
    }

    fn finalize_document(&mut self) {
        while !self.current.same_node(self.root) {
            self.current = self.finalize(self.current).unwrap();
        }

        self.finalize(self.root);
        self.process_inlines();
    }

    fn finalize(&mut self, node: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
        self.finalize_borrowed(node, &mut *node.data.borrow_mut())
    }

    fn finalize_borrowed(
        &mut self,
        node: &'a AstNode<'a>,
        ast: &mut Ast<'a>,
    ) -> Option<&'a AstNode<'a>> {
        assert!(ast.open);
        ast.open = false;

        if !self.linebuf.is_empty() {
            ast.end_line = self.line_number;
            ast.end_column = self.last_line_length;
        } else if match ast.value {
                   NodeValue::Document => true,
                   NodeValue::CodeBlock(ref ncb) => ncb.fenced,
                   NodeValue::Heading(ref nh) => nh.setext,
                   _ => false,
               }
        {
            ast.end_line = self.line_number;
            ast.end_column = self.linebuf.len();
            if ast.end_column > 0 && self.linebuf[ast.end_column - 1] == b'\n' {
                ast.end_column -= 1;
            }
            if ast.end_column > 0 && self.linebuf[ast.end_column - 1] == b'\r' {
                ast.end_column -= 1;
            }
        } else {
            ast.end_line = self.line_number - 1;
            ast.end_column = self.last_line_length;
        }

        let content = &mut ast.content;
        let mut pos = 0;

        let parent = node.parent();

        match ast.value {
            NodeValue::Paragraph => {
                while !content.is_empty() && content[0] == b'[' &&
                    unwrap_into(self.parse_reference_inline(content), &mut pos)
                {
                    while pos > 0 {
                        // TODO
                        content.to_mut().remove(0);
                        pos -= 1;
                    }
                }
                if strings::is_blank(content) {
                    node.detach();
                }
            }
            NodeValue::CodeBlock(ref mut ncb) => {
                if !ncb.fenced {
                    // TODO: this will always cause a copy. we should avoid if possible.
                    strings::remove_trailing_blank_lines(content);
                    content.to_mut().push(b'\n');
                } else {
                    let mut pos = 0;
                    while pos < content.len() {
                        if strings::is_line_end_char(content[pos]) {
                            break;
                        }
                        pos += 1;
                    }
                    assert!(pos < content.len());

                    let mut tmp = entity::unescape_html(&cow_range(content, ..pos));
                    strings::trim(&mut tmp);
                    strings::unescape(&mut tmp);
                    ncb.info = tmp;

                    if content[pos] == b'\r' {
                        pos += 1;
                    }
                    if content[pos] == b'\n' {
                        pos += 1;
                    }

                    // TODO
                    while pos > 0 {
                        content.to_mut().remove(0);
                        pos -= 1;
                    }
                }
                mem::swap(&mut ncb.literal, content);
                content.to_mut().clear();
            }
            NodeValue::HtmlBlock(ref mut nhb) => {
                mem::swap(&mut nhb.literal, content);
                content.to_mut().clear();
            }
            NodeValue::List(ref mut nl) => {
                nl.tight = true;
                let mut ch = node.first_child();

                while let Some(item) = ch {
                    if item.data.borrow().last_line_blank && item.next_sibling().is_some() {
                        nl.tight = false;
                        break;
                    }

                    let mut subch = item.first_child();
                    while let Some(subitem) = subch {
                        if nodes::ends_with_blank_line(subitem) &&
                            (item.next_sibling().is_some() || subitem.next_sibling().is_some())
                        {
                            nl.tight = false;
                            break;
                        }
                        subch = subitem.next_sibling();
                    }

                    if !nl.tight {
                        break;
                    }

                    ch = item.next_sibling();
                }
            }
            _ => (),
        }

        parent
    }

    fn process_inlines(&mut self) {
        self.process_inlines_node(self.root);
    }

    fn process_inlines_node(&mut self, node: &'a AstNode<'a>) {
        if node.data.borrow().value.contains_inlines() {
            self.parse_inlines(node);
        }

        for n in node.children() {
            self.process_inlines_node(n);
        }
    }

    fn parse_inlines(&mut self, node: &'a AstNode<'a>) {
        let delimiter_arena = Arena::new();
        let node_data = node.data.borrow();
        let mut content = node_data.content.clone();
        strings::rtrim(&mut content);
        let mut subj = inlines::Subject::new(
            self.arena,
            self.options,
            content,
            &mut self.refmap,
            &delimiter_arena,
        );

        while !subj.eof() && subj.parse_inline(node) {}

        subj.process_emphasis(None);

        while subj.pop_bracket() {}
    }

    fn postprocess_text_nodes(&mut self, node: &'a AstNode<'a>) {
        let mut nch = node.first_child();

        while let Some(n) = nch {
            let mut this_bracket = false;
            loop {
                match n.data.borrow_mut().value {
                    NodeValue::Text(ref mut root) => {
                        let ns = match n.next_sibling() {
                            Some(ns) => ns,
                            _ => {
                                self.postprocess_text_node(n, root);
                                break;
                            }
                        };

                        match ns.data.borrow().value {
                            NodeValue::Text(ref adj) => {
                                root.to_mut().extend_from_slice(adj);
                                ns.detach();
                            }
                            _ => {
                                self.postprocess_text_node(n, root);
                                break;
                            }
                        }
                    }
                    NodeValue::Link(..) |
                    NodeValue::Image(..) => {
                        this_bracket = true;
                        break;
                    }
                    _ => break,
                }
            }

            if !this_bracket {
                self.postprocess_text_nodes(n);
            }

            nch = n.next_sibling();
        }
    }

    fn postprocess_text_node(&mut self, node: &'a AstNode<'a>, text: &mut Cow<'a, [u8]>) {
        if self.options.ext_tasklist {
            self.process_tasklist(node, text);
        }

        if self.options.ext_autolink {
            autolink::process_autolinks(self.arena, node, text);
        }

    }

    fn process_tasklist(&mut self, node: &'a AstNode<'a>, text: &mut Cow<'a, [u8]>) {
        lazy_static! {
            static ref TASKLIST: Regex = Regex::new(r"\A(\s*\[([xX ])\])(?:\z|\s)").unwrap();
        }

        let (active, end) = match TASKLIST.captures(text) {
            None => return,
            Some(c) => (c.get(2).unwrap().as_bytes() != b" ", c.get(1).unwrap().end()),
        };

        let parent = node.parent().unwrap();
        if node.previous_sibling().is_some() || parent.previous_sibling().is_some() {
            return;
        }

        match parent.data.borrow().value {
            NodeValue::Paragraph => (),
            _ => return,
        }

        match parent.parent().unwrap().data.borrow().value {
            NodeValue::Item(..) => (),
            _ => return,
        }

        *text = text[end..].to_vec().into();
        let checkbox = inlines::make_inline(
            self.arena,
            NodeValue::HtmlInline(
                if active {
                    b"<input type=\"checkbox\" disabled=\"\" checked=\"\" />".to_vec().into()
                } else {
                    b"<input type=\"checkbox\" disabled=\"\" />".to_vec().into()
                }
            ),
        );
        node.insert_before(checkbox);
    }

    fn parse_reference_inline(&mut self, content: &Cow<'a, [u8]>) -> Option<usize> {
        let delimiter_arena = Arena::new();
        let mut subj = inlines::Subject::new(
            self.arena,
            self.options,
            content.clone(),
            &mut self.refmap,
            &delimiter_arena,
        );

        let mut lab: Vec<u8> = match subj.link_label() {
            Some(lab) => if lab.is_empty() { return None } else { lab },
            None => return None,
        }.to_vec();

        if subj.peek_char() != Some(&(b':')) {
            return None;
        }

        subj.pos += 1;
        subj.spnl();
        let matchlen = match inlines::manual_scan_link_url(&subj.input[subj.pos..]) {
            Some(matchlen) => matchlen,
            None => return None,
        };
        let url = cow_range(&subj.input, subj.pos..subj.pos + matchlen);
        subj.pos += matchlen;

        let beforetitle = subj.pos;
        subj.spnl();
        let title = match scanners::link_title(&subj.input[subj.pos..]) {
            Some(matchlen) => {
                let t = cow_range(&subj.input, subj.pos..subj.pos + matchlen);
                subj.pos += matchlen;
                t
            }
            _ => {
                subj.pos = beforetitle;
                Cow::from(vec![])
            }
        };

        subj.skip_spaces();
        if !subj.skip_line_end() {
            if !title.is_empty() {
                subj.pos = beforetitle;
                subj.skip_spaces();
                if !subj.skip_line_end() {
                    return None;
                }
            } else {
                return None;
            }
        }

        lab = strings::normalize_reference_label(&lab);
        if !lab.is_empty() {
            subj.refmap.entry(lab.to_vec()).or_insert(Reference {
                url: strings::clean_url(&url),
                title: strings::clean_title(&title),
            });
        }
        Some(subj.pos)
    }
}

enum AddTextResult {
    CodeBlock,
    HtmlBlock(u8),
    Otherwise,
}

fn parse_list_marker(
    line: &[u8],
    mut pos: usize,
    interrupts_paragraph: bool,
) -> Option<(usize, NodeList)> {
    let mut c = line[pos];
    let startpos = pos;

    if c == b'*' || c == b'-' || c == b'+' {
        pos += 1;
        if !isspace(line[pos]) {
            return None;
        }

        if interrupts_paragraph {
            let mut i = pos;
            while strings::is_space_or_tab(line[i]) {
                i += 1;
            }
            if line[i] == b'\n' {
                return None;
            }
        }

        return Some((
            pos - startpos,
            NodeList {
                list_type: ListType::Bullet,
                marker_offset: 0,
                padding: 0,
                start: 1,
                delimiter: ListDelimType::Period,
                bullet_char: c,
                tight: false,
            },
        ));
    } else if isdigit(c) {
        let mut start: usize = 0;
        let mut digits = 0;

        loop {
            start = (10 * start) + (line[pos] - b'0') as usize;
            pos += 1;
            digits += 1;

            if !(digits < 9 && isdigit(line[pos])) {
                break;
            }
        }

        if interrupts_paragraph && start != 1 {
            return None;
        }

        c = line[pos];
        if c != b'.' && c != b')' {
            return None;
        }

        pos += 1;

        if !isspace(line[pos]) {
            return None;
        }

        if interrupts_paragraph {
            let mut i = pos;
            while strings::is_space_or_tab(line[i]) {
                i += 1;
            }
            if strings::is_line_end_char(line[i]) {
                return None;
            }
        }

        return Some((
            pos - startpos,
            NodeList {
                list_type: ListType::Ordered,
                marker_offset: 0,
                padding: 0,
                start: start,
                delimiter: if c == b'.' {
                    ListDelimType::Period
                } else {
                    ListDelimType::Paren
                },
                bullet_char: 0,
                tight: false,
            },
        ));
    }

    None
}

pub fn unwrap_into<T>(t: Option<T>, out: &mut T) -> bool {
    match t {
        Some(v) => {
            *out = v;
            true
        }
        _ => false,
    }
}

pub fn unwrap_into_copy<T: Copy>(t: Option<&T>, out: &mut T) -> bool {
    match t {
        Some(v) => {
            *out = *v;
            true
        }
        _ => false,
    }
}

fn unwrap_into_2<T, U>(tu: Option<(T, U)>, out_t: &mut T, out_u: &mut U) -> bool {
    match tu {
        Some((t, u)) => {
            *out_t = t;
            *out_u = u;
            true
        }
        _ => false,
    }
}

fn lists_match(list_data: &NodeList, item_data: &NodeList) -> bool {
    list_data.list_type == item_data.list_type && list_data.delimiter == item_data.delimiter &&
        list_data.bullet_char == item_data.bullet_char
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutolinkType {
    URI,
    Email,
}

pub fn cow_range<'a, T, R>(cow: &Cow<'a, [T]>, range: R) -> Cow<'a, [T]>
    where T : Clone, [T] : Index<R, Output=[T]>, Vec<T> : Index<R, Output=[T]> {
    match *cow {
        Cow::Borrowed(p) => Cow::from(&p[range]),
        Cow::Owned(ref o) => Cow::from(o[range].to_vec()),
    }
}

fn cow_append<'a, T>(cow: &mut Cow<'a, [T]>, data: &[T])
    where T : Clone {
    // TODO: we could optimise this.
    cow.to_mut().extend_from_slice(data);
}
