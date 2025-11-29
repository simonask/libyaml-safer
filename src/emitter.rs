use std::collections::VecDeque;

use crate::macros::{
    is_alpha, is_ascii, is_blank, is_blankz, is_bom, is_break, is_breakz, is_printable, is_space,
};
use crate::{
    Break, Encoding, Error, Event, EventData, MappingStyle, Result, ScalarStyle, SequenceStyle,
    TagDirective, VersionDirective, OUTPUT_BUFFER_SIZE,
};

/// The emitter structure.
///
/// All members are internal. Manage the structure using the `yaml_emitter_`
/// family of functions.
#[non_exhaustive]
pub struct Emitter<'w> {
    /// Write handler.
    pub(crate) write_handler: Option<&'w mut dyn std::io::Write>,
    /// The working buffer.
    ///
    /// This always contains valid UTF-8.
    pub(crate) buffer: String,
    /// The raw buffer.
    ///
    /// This contains the output in the encoded format, so for example it may be
    /// UTF-16 encoded.
    pub(crate) raw_buffer: Vec<u8>,
    /// The stream encoding.
    pub(crate) encoding: Encoding,
    /// If the output is in the canonical style?
    pub(crate) canonical: bool,
    /// The number of indentation spaces.
    pub(crate) best_indent: i32,
    /// The preferred width of the output lines.
    pub(crate) best_width: i32,
    /// Allow unescaped non-ASCII characters?
    pub(crate) unicode: bool,
    /// The preferred line break.
    pub(crate) line_break: Break,
    /// The stack of states.
    pub(crate) states: Vec<EmitterState>,
    /// The current emitter state.
    pub(crate) state: EmitterState,
    /// The event queue.
    pub(crate) events: VecDeque<Event>,
    /// The stack of indentation levels.
    pub(crate) indents: Vec<i32>,
    /// The list of tag directives.
    pub(crate) tag_directives: Vec<TagDirective>,
    /// The current indentation level.
    pub(crate) indent: i32,
    /// The current flow level.
    pub(crate) flow_level: i32,
    /// Is it the document root context?
    pub(crate) root_context: bool,
    /// Is it a sequence context?
    pub(crate) sequence_context: bool,
    /// Is it a mapping context?
    pub(crate) mapping_context: bool,
    /// Is it a simple mapping key context?
    pub(crate) simple_key_context: bool,
    /// The current line.
    pub(crate) line: i32,
    /// The current column.
    pub(crate) column: i32,
    /// If the last character was a whitespace?
    pub(crate) whitespace: bool,
    /// If the last character was an indentation character (' ', '-', '?', ':')?
    pub(crate) indention: bool,
    /// If an explicit document end is required?
    pub(crate) open_ended: i32,
    /// If the stream was already opened?
    pub(crate) opened: bool,
    /// If the stream was already closed?
    pub(crate) closed: bool,
    /// The information associated with the document nodes.
    // Note: Same length as `document.nodes`.
    pub(crate) anchors: Vec<Anchors>,
    /// The last assigned anchor id.
    pub(crate) last_anchor_id: i32,
}

impl Default for Emitter<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The emitter states.
#[derive(Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[non_exhaustive]
pub enum EmitterState {
    /// Expect STREAM-START.
    #[default]
    StreamStart = 0,
    /// Expect the first DOCUMENT-START or STREAM-END.
    FirstDocumentStart = 1,
    /// Expect DOCUMENT-START or STREAM-END.
    DocumentStart = 2,
    /// Expect the content of a document.
    DocumentContent = 3,
    /// Expect DOCUMENT-END.
    DocumentEnd = 4,
    /// Expect the first item of a flow sequence.
    FlowSequenceFirstItem = 5,
    /// Expect an item of a flow sequence.
    FlowSequenceItem = 6,
    /// Expect the first key of a flow mapping.
    FlowMappingFirstKey = 7,
    /// Expect a key of a flow mapping.
    FlowMappingKey = 8,
    /// Expect a value for a simple key of a flow mapping.
    FlowMappingSimpleValue = 9,
    /// Expect a value of a flow mapping.
    FlowMappingValue = 10,
    /// Expect the first item of a block sequence.
    BlockSequenceFirstItem = 11,
    /// Expect an item of a block sequence.
    BlockSequenceItem = 12,
    /// Expect the first key of a block mapping.
    BlockMappingFirstKey = 13,
    /// Expect the key of a block mapping.
    BlockMappingKey = 14,
    /// Expect a value for a simple key of a block mapping.
    BlockMappingSimpleValue = 15,
    /// Expect a value of a block mapping.
    BlockMappingValue = 16,
    /// Expect nothing.
    End = 17,
}

#[derive(Copy, Clone, Default)]
pub(crate) struct Anchors {
    /// The number of references.
    pub references: i32,
    /// The anchor id.
    pub anchor: i32,
    /// If the node has been emitted?
    pub serialized: bool,
}

#[derive(Default)]
struct Analysis<'a> {
    pub anchor: Option<AnchorAnalysis<'a>>,
    pub tag: Option<TagAnalysis<'a>>,
    pub scalar: Option<ScalarAnalysis<'a>>,
}

struct AnchorAnalysis<'a> {
    pub anchor: &'a str,
    pub alias: bool,
}

struct TagAnalysis<'a> {
    pub handle: &'a str,
    pub suffix: &'a str,
}

struct ScalarAnalysis<'a> {
    /// The scalar value.
    pub value: &'a str,
    /// Does the scalar contain line breaks?
    pub multiline: bool,
    /// Can the scalar be expessed in the flow plain style?
    pub flow_plain_allowed: bool,
    /// Can the scalar be expressed in the block plain style?
    pub block_plain_allowed: bool,
    /// Can the scalar be expressed in the single quoted style?
    pub single_quoted_allowed: bool,
    /// Can the scalar be expressed in the literal or folded styles?
    pub block_allowed: bool,
    /// The output style.
    pub style: ScalarStyle,
}

impl<'w> Emitter<'w> {
    /// Create an self.
    pub fn new() -> Emitter<'w> {
        Emitter {
            write_handler: None,
            buffer: String::with_capacity(OUTPUT_BUFFER_SIZE),
            raw_buffer: Vec::with_capacity(OUTPUT_BUFFER_SIZE),
            encoding: Encoding::Any,
            canonical: false,
            best_indent: 0,
            best_width: 0,
            unicode: false,
            line_break: Break::default(),
            states: Vec::with_capacity(16),
            state: EmitterState::default(),
            events: VecDeque::with_capacity(16),
            indents: Vec::with_capacity(16),
            tag_directives: Vec::with_capacity(16),
            indent: 0,
            flow_level: 0,
            root_context: false,
            sequence_context: false,
            mapping_context: false,
            simple_key_context: false,
            line: 0,
            column: 0,
            whitespace: false,
            indention: false,
            open_ended: 0,
            opened: false,
            closed: false,
            anchors: Vec::new(),
            last_anchor_id: 0,
        }
    }

    /// Reset the emitter state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Start a YAML stream.
    ///
    /// This function should be used before
    /// [`Document::dump()`](crate::Document::dump) is called.
    pub fn open(&mut self) -> Result<()> {
        assert!(!self.opened);
        let event = Event::stream_start(Encoding::Any);
        self.emit(event)?;
        self.opened = true;
        Ok(())
    }

    /// Finish a YAML stream.
    ///
    /// This function should be used after
    /// [`Document::dump()`](crate::Document::dump) is called.
    pub fn close(&mut self) -> Result<()> {
        assert!(self.opened);
        if self.closed {
            return Ok(());
        }
        let event = Event::stream_end();
        self.emit(event)?;
        self.closed = true;
        Ok(())
    }

    /// Set a string output.
    ///
    /// The emitter will write the output characters to the `output` buffer.
    pub fn set_output_string(&mut self, output: &'w mut Vec<u8>) {
        assert!(self.write_handler.is_none());
        if self.encoding == Encoding::Any {
            self.set_encoding(Encoding::Utf8);
        } else if self.encoding != Encoding::Utf8 {
            panic!("cannot output UTF-16 to String")
        }
        output.clear();
        self.write_handler = Some(output);
    }

    /// Set a generic output handler.
    pub fn set_output(&mut self, handler: &'w mut dyn std::io::Write) {
        assert!(self.write_handler.is_none());
        self.write_handler = Some(handler);
    }

    /// Set the output encoding.
    pub fn set_encoding(&mut self, encoding: Encoding) {
        assert_eq!(self.encoding, Encoding::Any);
        self.encoding = encoding;
    }

    /// Set if the output should be in the "canonical" format as in the YAML
    /// specification.
    pub fn set_canonical(&mut self, canonical: bool) {
        self.canonical = canonical;
    }

    /// Set the indentation increment.
    pub fn set_indent(&mut self, indent: i32) {
        self.best_indent = if 1 < indent && indent < 10 { indent } else { 2 };
    }

    /// Set the preferred line width. -1 means unlimited.
    pub fn set_width(&mut self, width: i32) {
        self.best_width = if width >= 0 { width } else { -1 };
    }

    /// Set if unescaped non-ASCII characters are allowed.
    pub fn set_unicode(&mut self, unicode: bool) {
        self.unicode = unicode;
    }

    /// Set the preferred line break.
    pub fn set_break(&mut self, line_break: Break) {
        self.line_break = line_break;
    }

    /// Emit an event.
    ///
    /// The event object may be generated using the
    /// [`Parser::parse()`](crate::Parser::parse) function. The emitter takes
    /// the responsibility for the event object and destroys its content after
    /// it is emitted. The event object is destroyed even if the function fails.
    pub fn emit(&mut self, event: Event) -> Result<()> {
        self.events.push_back(event);
        while let Some(event) = self.needs_mode_events() {
            let tag_directives = core::mem::take(&mut self.tag_directives);

            let mut analysis = self.analyze_event(&event, &tag_directives)?;
            self.state_machine(&event, &mut analysis)?;

            // The DOCUMENT-START event populates the tag directives, and this
            // happens only once, so don't swap out the tags in that case.
            if self.tag_directives.is_empty() {
                self.tag_directives = tag_directives;
            }
        }
        Ok(())
    }

    /// Equivalent of the libyaml `FLUSH` macro.
    fn flush_if_needed(&mut self) -> Result<()> {
        if self.buffer.len() < OUTPUT_BUFFER_SIZE - 5 {
            Ok(())
        } else {
            self.flush()
        }
    }

    /// Equivalent of the libyaml `PUT` macro.
    fn put(&mut self, value: char) -> Result<()> {
        self.flush_if_needed()?;
        self.buffer.push(value);
        self.column += 1;
        Ok(())
    }

    /// Equivalent of the libyaml `PUT_BREAK` macro.
    fn put_break(&mut self) -> Result<()> {
        self.flush_if_needed()?;
        if self.line_break == Break::Cr {
            self.buffer.push('\r');
        } else if self.line_break == Break::Ln {
            self.buffer.push('\n');
        } else if self.line_break == Break::CrLn {
            self.buffer.push_str("\r\n");
        }
        self.column = 0;
        self.line += 1;
        Ok(())
    }

    /// Write UTF-8 charanters from `string` to `emitter` and increment
    /// `emitter.column` the appropriate number of times. It is assumed that the
    /// string does not contain line breaks!
    fn write_str(&mut self, string: &str) -> Result<()> {
        if self.buffer.len() + string.len() > OUTPUT_BUFFER_SIZE {
            self.flush()?;
        }

        // Note: Reserves less than what is necessary if there are UTF-8
        // characters present.
        self.buffer.reserve(string.len());

        self.column += string.chars().count() as i32;

        // Note: This may cause the buffer to become slightly larger than
        // `OUTPUT_BUFFER_SIZE`, but not by much.
        self.buffer.push_str(string);

        Ok(())
    }

    /// Equivalent of the libyaml `WRITE` macro.
    fn write_char(&mut self, ch: char) -> Result<()> {
        self.flush_if_needed()?;
        self.buffer.push(ch);
        self.column += 1;
        Ok(())
    }

    /// Equivalent of the libyaml `WRITE_BREAK` macro.
    fn write_break(&mut self, ch: char) -> Result<()> {
        self.flush_if_needed()?;
        if ch == '\n' {
            self.put_break()?;
        } else {
            self.write_char(ch)?;
            self.column = 0;
            self.line += 1;
        }
        Ok(())
    }

    fn needs_mode_events(&mut self) -> Option<Event> {
        let first = self.events.front()?;

        let accummulate = match &first.data {
            EventData::DocumentStart { .. } => 1,
            EventData::SequenceStart { .. } => 2,
            EventData::MappingStart { .. } => 3,
            _ => return self.events.pop_front(),
        };

        if self.events.len() > accummulate {
            return self.events.pop_front();
        }

        let mut level = 0;
        for event in &self.events {
            match event.data {
                EventData::StreamStart { .. }
                | EventData::DocumentStart { .. }
                | EventData::SequenceStart { .. }
                | EventData::MappingStart { .. } => {
                    level += 1;
                }

                EventData::StreamEnd
                | EventData::DocumentEnd { .. }
                | EventData::SequenceEnd
                | EventData::MappingEnd => {
                    level -= 1;
                }
                _ => {}
            }

            if level == 0 {
                return self.events.pop_front();
            }
        }

        None
    }

    fn append_tag_directive(&mut self, value: TagDirective, allow_duplicates: bool) -> Result<()> {
        for tag_directive in &self.tag_directives {
            if value.handle == tag_directive.handle {
                if allow_duplicates {
                    return Ok(());
                }
                return Err(Error::emitter("duplicate %TAG directive"));
            }
        }
        self.tag_directives.push(value);
        Ok(())
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        self.indents.push(self.indent);
        if self.indent < 0 {
            self.indent = if flow { self.best_indent } else { 0 };
        } else if !indentless {
            self.indent += self.best_indent;
        }
    }

    fn state_machine<'a>(&mut self, event: &'a Event, analysis: &mut Analysis<'a>) -> Result<()> {
        match self.state {
            EmitterState::StreamStart => self.emit_stream_start(event),
            EmitterState::FirstDocumentStart => self.emit_document_start(event, true),
            EmitterState::DocumentStart => self.emit_document_start(event, false),
            EmitterState::DocumentContent => self.emit_document_content(event, analysis),
            EmitterState::DocumentEnd => self.emit_document_end(event),
            EmitterState::FlowSequenceFirstItem => {
                self.emit_flow_sequence_item(event, true, analysis)
            }
            EmitterState::FlowSequenceItem => self.emit_flow_sequence_item(event, false, analysis),
            EmitterState::FlowMappingFirstKey => self.emit_flow_mapping_key(event, true, analysis),
            EmitterState::FlowMappingKey => self.emit_flow_mapping_key(event, false, analysis),
            EmitterState::FlowMappingSimpleValue => {
                self.emit_flow_mapping_value(event, true, analysis)
            }
            EmitterState::FlowMappingValue => self.emit_flow_mapping_value(event, false, analysis),
            EmitterState::BlockSequenceFirstItem => {
                self.emit_block_sequence_item(event, true, analysis)
            }
            EmitterState::BlockSequenceItem => {
                self.emit_block_sequence_item(event, false, analysis)
            }
            EmitterState::BlockMappingFirstKey => {
                self.emit_block_mapping_key(event, true, analysis)
            }
            EmitterState::BlockMappingKey => self.emit_block_mapping_key(event, false, analysis),
            EmitterState::BlockMappingSimpleValue => {
                self.emit_block_mapping_value(event, true, analysis)
            }
            EmitterState::BlockMappingValue => {
                self.emit_block_mapping_value(event, false, analysis)
            }
            EmitterState::End => Err(Error::emitter("expected nothing after STREAM-END")),
        }
    }

    fn emit_stream_start(&mut self, event: &Event) -> Result<()> {
        self.open_ended = 0;
        if let EventData::StreamStart { ref encoding } = event.data {
            if self.encoding == Encoding::Any {
                self.encoding = *encoding;
            }
            if self.encoding == Encoding::Any {
                self.encoding = Encoding::Utf8;
            }
            if self.best_indent < 2 || self.best_indent > 9 {
                self.best_indent = 2;
            }
            if self.best_width >= 0 && self.best_width <= self.best_indent * 2 {
                self.best_width = 80;
            }
            if self.best_width < 0 {
                self.best_width = i32::MAX;
            }
            if self.line_break == Break::Any {
                self.line_break = Break::Ln;
            }
            self.indent = -1;
            self.line = 0;
            self.column = 0;
            self.whitespace = true;
            self.indention = true;
            if self.encoding != Encoding::Utf8 {
                self.write_bom()?;
            }
            self.state = EmitterState::FirstDocumentStart;
            return Ok(());
        }
        Err(Error::emitter("expected STREAM-START"))
    }

    fn emit_document_start(&mut self, event: &Event, first: bool) -> Result<()> {
        if let EventData::DocumentStart {
            version_directive,
            tag_directives,
            implicit,
        } = &event.data
        {
            let default_tag_directives: [TagDirective; 2] = [
                // TODO: Avoid these heap allocations.
                TagDirective {
                    handle: String::from("!"),
                    prefix: String::from("!"),
                },
                TagDirective {
                    handle: String::from("!!"),
                    prefix: String::from("tag:yaml.org,2002:"),
                },
            ];
            let mut implicit = *implicit;
            if let Some(version_directive) = version_directive {
                Self::analyze_version_directive(*version_directive)?;
            }
            for tag_directive in tag_directives {
                Self::analyze_tag_directive(tag_directive)?;
                self.append_tag_directive(tag_directive.clone(), false)?;
            }
            for tag_directive in default_tag_directives {
                self.append_tag_directive(tag_directive, true)?;
            }
            if !first || self.canonical {
                implicit = false;
            }
            if (version_directive.is_some() || !tag_directives.is_empty()) && self.open_ended != 0 {
                self.write_indicator("...", true, false, false)?;
                self.write_indent()?;
            }
            self.open_ended = 0;
            if let Some(version_directive) = version_directive {
                implicit = false;
                self.write_indicator("%YAML", true, false, false)?;
                if version_directive.minor == 1 {
                    self.write_indicator("1.1", true, false, false)?;
                } else {
                    self.write_indicator("1.2", true, false, false)?;
                }
                self.write_indent()?;
            }
            if !tag_directives.is_empty() {
                implicit = false;
                for tag_directive in tag_directives {
                    self.write_indicator("%TAG", true, false, false)?;
                    self.write_tag_handle(&tag_directive.handle)?;
                    self.write_tag_content(&tag_directive.prefix, true)?;
                    self.write_indent()?;
                }
            }
            if Self::check_empty_document() {
                implicit = false;
            }
            if !implicit {
                self.write_indent()?;
                self.write_indicator("---", true, false, false)?;
                if self.canonical {
                    self.write_indent()?;
                }
            }
            self.state = EmitterState::DocumentContent;
            self.open_ended = 0;
            return Ok(());
        } else if let EventData::StreamEnd = &event.data {
            if self.open_ended == 2 {
                self.write_indicator("...", true, false, false)?;
                self.open_ended = 0;
                self.write_indent()?;
            }
            self.flush()?;
            self.state = EmitterState::End;
            return Ok(());
        }

        Err(Error::emitter("expected DOCUMENT-START or STREAM-END"))
    }

    fn emit_document_content(&mut self, event: &Event, analysis: &mut Analysis) -> Result<()> {
        self.states.push(EmitterState::DocumentEnd);
        self.emit_node(event, true, false, false, false, analysis)
    }

    fn emit_document_end(&mut self, event: &Event) -> Result<()> {
        if let EventData::DocumentEnd { implicit } = &event.data {
            let implicit = *implicit;
            self.write_indent()?;
            if !implicit {
                self.write_indicator("...", true, false, false)?;
                self.open_ended = 0;
                self.write_indent()?;
            } else if self.open_ended == 0 {
                self.open_ended = 1;
            }
            self.flush()?;
            self.state = EmitterState::DocumentStart;
            self.tag_directives.clear();
            return Ok(());
        }

        Err(Error::emitter("expected DOCUMENT-END"))
    }

    fn emit_flow_sequence_item(
        &mut self,
        event: &Event,
        first: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if first {
            self.write_indicator("[", true, true, false)?;
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if let EventData::SequenceEnd = &event.data {
            self.flow_level -= 1;
            self.indent = self.indents.pop().unwrap();
            if self.canonical && !first {
                self.write_indicator(",", false, false, false)?;
                self.write_indent()?;
            }
            self.write_indicator("]", false, false, false)?;
            self.state = self.states.pop().unwrap();
            return Ok(());
        }
        if !first {
            self.write_indicator(",", false, false, false)?;
        }
        if self.canonical || self.column > self.best_width {
            self.write_indent()?;
        }
        self.states.push(EmitterState::FlowSequenceItem);
        self.emit_node(event, false, true, false, false, analysis)
    }

    fn emit_flow_mapping_key(
        &mut self,
        event: &Event,
        first: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if first {
            self.write_indicator("{", true, true, false)?;
            self.increase_indent(true, false);
            self.flow_level += 1;
        }
        if let EventData::MappingEnd = &event.data {
            assert!(!self.indents.is_empty(), "self.indents should not be empty");
            self.flow_level -= 1;
            self.indent = self.indents.pop().unwrap();
            if self.canonical && !first {
                self.write_indicator(",", false, false, false)?;
                self.write_indent()?;
            }
            self.write_indicator("}", false, false, false)?;
            self.state = self.states.pop().unwrap();
            return Ok(());
        }
        if !first {
            self.write_indicator(",", false, false, false)?;
        }
        if self.canonical || self.column > self.best_width {
            self.write_indent()?;
        }
        if !self.canonical && self.check_simple_key(event, analysis) {
            self.states.push(EmitterState::FlowMappingSimpleValue);
            self.emit_node(event, false, false, true, true, analysis)
        } else {
            self.write_indicator("?", true, false, false)?;
            self.states.push(EmitterState::FlowMappingValue);
            self.emit_node(event, false, false, true, false, analysis)
        }
    }

    fn emit_flow_mapping_value(
        &mut self,
        event: &Event,
        simple: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if simple {
            self.write_indicator(":", false, false, false)?;
        } else {
            if self.canonical || self.column > self.best_width {
                self.write_indent()?;
            }
            self.write_indicator(":", true, false, false)?;
        }
        self.states.push(EmitterState::FlowMappingKey);
        self.emit_node(event, false, false, true, false, analysis)
    }

    fn emit_block_sequence_item(
        &mut self,
        event: &Event,
        first: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if first {
            self.increase_indent(false, self.mapping_context && !self.indention);
        }
        if let EventData::SequenceEnd = &event.data {
            self.indent = self.indents.pop().unwrap();
            self.state = self.states.pop().unwrap();
            return Ok(());
        }
        self.write_indent()?;
        self.write_indicator("-", true, false, true)?;
        self.states.push(EmitterState::BlockSequenceItem);
        self.emit_node(event, false, true, false, false, analysis)
    }

    fn emit_block_mapping_key(
        &mut self,
        event: &Event,
        first: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if first {
            self.increase_indent(false, false);
        }
        if let EventData::MappingEnd = &event.data {
            self.indent = self.indents.pop().unwrap();
            self.state = self.states.pop().unwrap();
            return Ok(());
        }
        self.write_indent()?;
        if self.check_simple_key(event, analysis) {
            self.states.push(EmitterState::BlockMappingSimpleValue);
            self.emit_node(event, false, false, true, true, analysis)
        } else {
            self.write_indicator("?", true, false, true)?;
            self.states.push(EmitterState::BlockMappingValue);
            self.emit_node(event, false, false, true, false, analysis)
        }
    }

    fn emit_block_mapping_value(
        &mut self,
        event: &Event,
        simple: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        if simple {
            self.write_indicator(":", false, false, false)?;
        } else {
            self.write_indent()?;
            self.write_indicator(":", true, false, true)?;
        }
        self.states.push(EmitterState::BlockMappingKey);
        self.emit_node(event, false, false, true, false, analysis)
    }

    fn emit_node(
        &mut self,
        event: &Event,
        root: bool,
        sequence: bool,
        mapping: bool,
        simple_key: bool,
        analysis: &mut Analysis,
    ) -> Result<()> {
        self.root_context = root;
        self.sequence_context = sequence;
        self.mapping_context = mapping;
        self.simple_key_context = simple_key;

        match event.data {
            EventData::Alias { .. } => self.emit_alias(event, analysis.anchor.as_ref()),
            EventData::Scalar { .. } => self.emit_scalar(event, analysis),
            EventData::SequenceStart { .. } => self.emit_sequence_start(event, analysis),
            EventData::MappingStart { .. } => self.emit_mapping_start(event, analysis),
            _ => Err(Error::emitter(
                "expected SCALAR, SEQUENCE-START, MAPPING-START, or ALIAS",
            )),
        }
    }

    fn emit_alias(&mut self, _event: &Event, analysis: Option<&AnchorAnalysis>) -> Result<()> {
        self.process_anchor(analysis)?;
        if self.simple_key_context {
            self.put(' ')?;
        }
        self.state = self.states.pop().unwrap();
        Ok(())
    }

    fn emit_scalar(&mut self, event: &Event, analysis: &mut Analysis) -> Result<()> {
        let (anchor, tag, scalar) = match analysis {
            Analysis {
                anchor,
                tag,
                scalar: Some(scalar),
            } => (anchor, tag, scalar),
            _ => unreachable!("no scalar analysis"),
        };

        self.select_scalar_style(event, scalar, tag)?;
        self.process_anchor(anchor.as_ref())?;
        self.process_tag(tag.as_ref())?;
        self.increase_indent(true, false);
        self.process_scalar(scalar)?;
        self.indent = self.indents.pop().unwrap();
        self.state = self.states.pop().unwrap();
        Ok(())
    }

    fn emit_sequence_start(&mut self, event: &Event, analysis: &Analysis) -> Result<()> {
        let Analysis { anchor, tag, .. } = analysis;
        self.process_anchor(anchor.as_ref())?;
        self.process_tag(tag.as_ref())?;

        let style = match &event.data {
            EventData::SequenceStart { style, .. } => style,
            _ => unreachable!(),
        };

        if self.flow_level != 0
            || self.canonical
            || *style == SequenceStyle::Flow
            || self.check_empty_sequence(event)
        {
            self.state = EmitterState::FlowSequenceFirstItem;
        } else {
            self.state = EmitterState::BlockSequenceFirstItem;
        }
        Ok(())
    }

    fn emit_mapping_start(&mut self, event: &Event, analysis: &Analysis) -> Result<()> {
        let Analysis { anchor, tag, .. } = analysis;
        self.process_anchor(anchor.as_ref())?;
        self.process_tag(tag.as_ref())?;

        let style = match &event.data {
            EventData::MappingStart { style, .. } => style,
            _ => unreachable!(),
        };

        if self.flow_level != 0
            || self.canonical
            || *style == MappingStyle::Flow
            || self.check_empty_mapping(event)
        {
            self.state = EmitterState::FlowMappingFirstKey;
        } else {
            self.state = EmitterState::BlockMappingFirstKey;
        }
        Ok(())
    }

    fn check_empty_document() -> bool {
        false
    }

    fn check_empty_sequence(&self, event: &Event) -> bool {
        if self.events.is_empty() {
            return false;
        }
        let start = matches!(event.data, EventData::SequenceStart { .. });
        let end = matches!(self.events[0].data, EventData::SequenceEnd);
        start && end
    }

    fn check_empty_mapping(&self, event: &Event) -> bool {
        if self.events.is_empty() {
            return false;
        }
        let start = matches!(event.data, EventData::MappingStart { .. });
        let end = matches!(self.events[0].data, EventData::MappingEnd);
        start && end
    }

    fn check_simple_key(&self, event: &Event, analysis: &Analysis) -> bool {
        let Analysis {
            tag,
            anchor,
            scalar,
        } = analysis;

        let mut length = anchor.as_ref().map_or(0, |a| a.anchor.len())
            + tag.as_ref().map_or(0, |t| t.handle.len() + t.suffix.len());

        match event.data {
            EventData::Alias { .. } => {
                length = analysis.anchor.as_ref().map_or(0, |a| a.anchor.len());
            }
            EventData::Scalar { .. } => {
                let scalar = match scalar {
                    Some(scalar) => scalar,
                    None => panic!("no analysis for scalar"),
                };

                if scalar.multiline {
                    return false;
                }
                length += scalar.value.len();
            }
            EventData::SequenceStart { .. } => {
                if !self.check_empty_sequence(event) {
                    return false;
                }
            }
            EventData::MappingStart { .. } => {
                if !self.check_empty_mapping(event) {
                    return false;
                }
            }
            _ => return false,
        }

        if length > 128 {
            return false;
        }

        true
    }

    fn select_scalar_style(
        &mut self,
        event: &Event,
        scalar_analysis: &mut ScalarAnalysis,
        tag_analysis: &mut Option<TagAnalysis>,
    ) -> Result<()> {
        let (plain_implicit, quoted_implicit, style) = match &event.data {
            EventData::Scalar {
                plain_implicit,
                quoted_implicit,
                style,
                ..
            } => (plain_implicit, quoted_implicit, style),
            _ => unreachable!(),
        };

        let mut style: ScalarStyle = *style;
        let no_tag = tag_analysis.is_none();
        if no_tag && !*plain_implicit && !*quoted_implicit {
            return Err(Error::emitter(
                "neither tag nor implicit flags are specified",
            ));
        }
        if style == ScalarStyle::Any {
            style = ScalarStyle::Plain;
        }
        if self.canonical {
            style = ScalarStyle::DoubleQuoted;
        }
        if self.simple_key_context && scalar_analysis.multiline {
            style = ScalarStyle::DoubleQuoted;
        }
        if style == ScalarStyle::Plain {
            if self.flow_level != 0 && !scalar_analysis.flow_plain_allowed
                || self.flow_level == 0 && !scalar_analysis.block_plain_allowed
            {
                style = ScalarStyle::SingleQuoted;
            }
            if scalar_analysis.value.is_empty() && (self.flow_level != 0 || self.simple_key_context)
            {
                style = ScalarStyle::SingleQuoted;
            }
            if no_tag && !*plain_implicit {
                style = ScalarStyle::SingleQuoted;
            }
        }
        if style == ScalarStyle::SingleQuoted && !scalar_analysis.single_quoted_allowed {
            style = ScalarStyle::DoubleQuoted;
        }
        if (style == ScalarStyle::Literal || style == ScalarStyle::Folded)
            && (!scalar_analysis.block_allowed || self.flow_level != 0 || self.simple_key_context)
        {
            style = ScalarStyle::DoubleQuoted;
        }
        if no_tag && !*quoted_implicit && style != ScalarStyle::Plain {
            *tag_analysis = Some(TagAnalysis {
                handle: "!",
                suffix: "",
            });
        }
        scalar_analysis.style = style;
        Ok(())
    }

    fn process_anchor(&mut self, analysis: Option<&AnchorAnalysis>) -> Result<()> {
        let analysis = match analysis.as_ref() {
            Some(analysis) => analysis,
            None => return Ok(()),
        };
        self.write_indicator(if analysis.alias { "*" } else { "&" }, true, false, false)?;
        self.write_anchor(analysis.anchor)
    }

    fn process_tag(&mut self, analysis: Option<&TagAnalysis>) -> Result<()> {
        let analysis = match analysis {
            Some(analysis) => analysis,
            None => return Ok(()),
        };

        if analysis.handle.is_empty() && analysis.suffix.is_empty() {
            return Ok(());
        }
        if analysis.handle.is_empty() {
            self.write_indicator("!<", true, false, false)?;
            self.write_tag_content(analysis.suffix, false)?;
            self.write_indicator(">", false, false, false)?;
        } else {
            self.write_tag_handle(analysis.handle)?;
            if !analysis.suffix.is_empty() {
                self.write_tag_content(analysis.suffix, false)?;
            }
        }
        Ok(())
    }

    fn process_scalar(&mut self, analysis: &ScalarAnalysis) -> Result<()> {
        match analysis.style {
            ScalarStyle::Plain => self.write_plain_scalar(analysis.value, !self.simple_key_context),
            ScalarStyle::SingleQuoted => {
                self.write_single_quoted_scalar(analysis.value, !self.simple_key_context)
            }
            ScalarStyle::DoubleQuoted => {
                self.write_double_quoted_scalar(analysis.value, !self.simple_key_context)
            }
            ScalarStyle::Literal => self.write_literal_scalar(analysis.value),
            ScalarStyle::Folded => self.write_folded_scalar(analysis.value),
            ScalarStyle::Any => unreachable!("No scalar style chosen"),
        }
    }

    fn analyze_version_directive(version_directive: VersionDirective) -> Result<()> {
        if version_directive.major != 1
            || version_directive.minor != 1 && version_directive.minor != 2
        {
            return Err(Error::emitter("incompatible %YAML directive"));
        }
        Ok(())
    }

    fn analyze_tag_directive(tag_directive: &TagDirective) -> Result<()> {
        if tag_directive.handle.is_empty() {
            return Err(Error::emitter("tag handle must not be empty"));
        }
        if !tag_directive.handle.starts_with('!') {
            return Err(Error::emitter("tag handle must start with '!'"));
        }
        if !tag_directive.handle.ends_with('!') {
            return Err(Error::emitter("tag handle must end with '!'"));
        }
        if tag_directive.handle.len() > 2 {
            let tag_content = &tag_directive.handle[1..tag_directive.handle.len() - 1];
            for ch in tag_content.chars() {
                if !is_alpha(ch) {
                    return Err(Error::emitter(
                        "tag handle must contain alphanumerical characters only",
                    ));
                }
            }
        }

        if tag_directive.prefix.is_empty() {
            return Err(Error::emitter("tag prefix must not be empty"));
        }

        Ok(())
    }

    fn analyze_anchor(anchor: &str, alias: bool) -> Result<AnchorAnalysis<'_>> {
        if anchor.is_empty() {
            return Err(Error::emitter(if alias {
                "alias value must not be empty"
            } else {
                "anchor value must not be empty"
            }));
        }

        for ch in anchor.chars() {
            if !is_alpha(ch) {
                return Err(Error::emitter(if alias {
                    "alias value must contain alphanumerical characters only"
                } else {
                    "anchor value must contain alphanumerical characters only"
                }));
            }
        }

        Ok(AnchorAnalysis { anchor, alias })
    }

    fn analyze_tag<'a>(
        tag: &'a str,
        tag_directives: &'a [TagDirective],
    ) -> Result<TagAnalysis<'a>> {
        if tag.is_empty() {
            return Err(Error::emitter("tag value must not be empty"));
        }

        let mut handle = "";
        let mut suffix = tag;

        for tag_directive in tag_directives {
            let prefix_len = tag_directive.prefix.len();
            if prefix_len < tag.len() && tag_directive.prefix == tag[0..prefix_len] {
                handle = &tag_directive.handle;
                suffix = &tag[prefix_len..];
                break;
            }
        }

        Ok(TagAnalysis { handle, suffix })
    }

    fn analyze_scalar<'a>(&mut self, value: &'a str) -> Result<ScalarAnalysis<'a>> {
        let mut block_indicators = false;
        let mut flow_indicators = false;
        let mut line_breaks = false;
        let mut special_characters = false;
        let mut leading_space = false;
        let mut leading_break = false;
        let mut trailing_space = false;
        let mut trailing_break = false;
        let mut break_space = false;
        let mut space_break = false;
        let mut preceded_by_whitespace;
        let mut previous_space = false;
        let mut previous_break = false;

        if value.is_empty() {
            return Ok(ScalarAnalysis {
                value: "",
                multiline: false,
                flow_plain_allowed: false,
                block_plain_allowed: true,
                single_quoted_allowed: true,
                block_allowed: false,
                style: ScalarStyle::Any,
            });
        }

        if value.starts_with("---") || value.starts_with("...") {
            block_indicators = true;
            flow_indicators = true;
        }
        preceded_by_whitespace = true;

        let mut chars = value.chars();
        let mut first = true;

        while let Some(ch) = chars.next() {
            let next = chars.clone().next();
            let followed_by_whitespace = is_blankz(next);
            if first {
                match ch {
                    '#' | ',' | '[' | ']' | '{' | '}' | '&' | '*' | '!' | '|' | '>' | '\''
                    | '"' | '%' | '@' | '`' => {
                        flow_indicators = true;
                        block_indicators = true;
                    }
                    '?' | ':' => {
                        flow_indicators = true;
                        if followed_by_whitespace {
                            block_indicators = true;
                        }
                    }
                    '-' if followed_by_whitespace => {
                        flow_indicators = true;
                        block_indicators = true;
                    }
                    _ => {}
                }
            } else {
                match ch {
                    ',' | '?' | '[' | ']' | '{' | '}' => {
                        flow_indicators = true;
                    }
                    ':' => {
                        flow_indicators = true;
                        if followed_by_whitespace {
                            block_indicators = true;
                        }
                    }
                    '#' if preceded_by_whitespace => {
                        flow_indicators = true;
                        block_indicators = true;
                    }
                    _ => {}
                }
            }

            if !is_printable(ch) || !is_ascii(ch) && !self.unicode {
                special_characters = true;
            }
            if is_break(ch) {
                line_breaks = true;
            }

            if is_space(ch) {
                if first {
                    leading_space = true;
                }
                if next.is_none() {
                    trailing_space = true;
                }
                if previous_break {
                    break_space = true;
                }
                previous_space = true;
                previous_break = false;
            } else if is_break(ch) {
                if first {
                    leading_break = true;
                }
                if next.is_none() {
                    trailing_break = true;
                }
                if previous_space {
                    space_break = true;
                }
                previous_space = false;
                previous_break = true;
            } else {
                previous_space = false;
                previous_break = false;
            }

            preceded_by_whitespace = is_blankz(ch);
            first = false;
        }

        let mut analysis = ScalarAnalysis {
            value,
            multiline: line_breaks,
            flow_plain_allowed: true,
            block_plain_allowed: true,
            single_quoted_allowed: true,
            block_allowed: true,
            style: ScalarStyle::Any,
        };

        analysis.multiline = line_breaks;
        analysis.flow_plain_allowed = true;
        analysis.block_plain_allowed = true;
        analysis.single_quoted_allowed = true;
        analysis.block_allowed = true;
        if leading_space || leading_break || trailing_space || trailing_break {
            analysis.flow_plain_allowed = false;
            analysis.block_plain_allowed = false;
        }
        if trailing_space {
            analysis.block_allowed = false;
        }
        if break_space {
            analysis.flow_plain_allowed = false;
            analysis.block_plain_allowed = false;
            analysis.single_quoted_allowed = false;
        }
        if space_break || special_characters {
            analysis.flow_plain_allowed = false;
            analysis.block_plain_allowed = false;
            analysis.single_quoted_allowed = false;
            analysis.block_allowed = false;
        }
        if line_breaks {
            analysis.flow_plain_allowed = false;
            analysis.block_plain_allowed = false;
        }
        if flow_indicators {
            analysis.flow_plain_allowed = false;
        }
        if block_indicators {
            analysis.block_plain_allowed = false;
        }
        Ok(analysis)
    }

    fn analyze_event<'a>(
        &mut self,
        event: &'a Event,
        tag_directives: &'a [TagDirective],
    ) -> Result<Analysis<'a>> {
        let mut analysis = Analysis::default();

        match &event.data {
            EventData::Alias { anchor } => {
                analysis.anchor = Some(Self::analyze_anchor(anchor, true)?);
            }
            EventData::Scalar {
                anchor,
                tag,
                value,
                plain_implicit,
                quoted_implicit,
                ..
            } => {
                let (plain_implicit, quoted_implicit) = (*plain_implicit, *quoted_implicit);
                if let Some(anchor) = anchor {
                    analysis.anchor = Some(Self::analyze_anchor(anchor, false)?);
                }
                if tag.is_some() && (self.canonical || !plain_implicit && !quoted_implicit) {
                    analysis.tag =
                        Some(Self::analyze_tag(tag.as_deref().unwrap(), tag_directives)?);
                }
                analysis.scalar = Some(self.analyze_scalar(value)?);
            }
            EventData::SequenceStart {
                anchor,
                tag,
                implicit,
                ..
            }
            | EventData::MappingStart {
                anchor,
                tag,
                implicit,
                ..
            } => {
                if let Some(anchor) = anchor {
                    analysis.anchor = Some(Self::analyze_anchor(anchor, false)?);
                }
                if tag.is_some() && (self.canonical || !*implicit) {
                    analysis.tag =
                        Some(Self::analyze_tag(tag.as_deref().unwrap(), tag_directives)?);
                }
            }
            _ => {}
        }

        Ok(analysis)
    }

    fn write_bom(&mut self) -> Result<()> {
        self.flush_if_needed()?;
        self.buffer.push('\u{feff}');
        Ok(())
    }

    fn write_indent(&mut self) -> Result<()> {
        let indent = if self.indent >= 0 { self.indent } else { 0 };
        if !self.indention || self.column > indent || self.column == indent && !self.whitespace {
            self.put_break()?;
        }
        while self.column < indent {
            self.put(' ')?;
        }
        self.whitespace = true;
        self.indention = true;
        Ok(())
    }

    fn write_indicator(
        &mut self,
        indicator: &str,
        need_whitespace: bool,
        is_whitespace: bool,
        is_indention: bool,
    ) -> Result<()> {
        if need_whitespace && !self.whitespace {
            self.put(' ')?;
        }
        self.write_str(indicator)?;
        self.whitespace = is_whitespace;
        self.indention = self.indention && is_indention;
        Ok(())
    }

    fn write_anchor(&mut self, value: &str) -> Result<()> {
        self.write_str(value)?;
        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_tag_handle(&mut self, value: &str) -> Result<()> {
        if !self.whitespace {
            self.put(' ')?;
        }
        self.write_str(value)?;
        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_tag_content(&mut self, value: &str, need_whitespace: bool) -> Result<()> {
        if need_whitespace && !self.whitespace {
            self.put(' ')?;
        }

        for ch in value.chars() {
            if is_alpha(ch) {
                self.write_char(ch)?;
                continue;
            }

            match ch {
                ';' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | ',' | '_' | '.' | '~'
                | '*' | '\'' | '(' | ')' | '[' | ']' => {
                    self.write_char(ch)?;
                    continue;
                }
                _ => {}
            }

            // URI escape
            let mut encode_buffer = [0u8; 4];
            let encoded_char = ch.encode_utf8(&mut encode_buffer);
            for value in encoded_char.bytes() {
                let upper = char::from_digit(value as u32 >> 4, 16)
                    .expect("invalid digit")
                    .to_ascii_uppercase();
                let lower = char::from_digit(value as u32 & 0x0F, 16)
                    .expect("invalid digit")
                    .to_ascii_uppercase();
                self.put('%')?;
                self.put(upper)?;
                self.put(lower)?;
            }
        }

        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_plain_scalar(&mut self, value: &str, allow_breaks: bool) -> Result<()> {
        let mut spaces = false;
        let mut breaks = false;
        if !self.whitespace && (!value.is_empty() || self.flow_level != 0) {
            self.put(' ')?;
        }

        let mut chars = value.chars();

        while let Some(ch) = chars.next() {
            let next = chars.clone().next();
            if is_space(ch) {
                if allow_breaks && !spaces && self.column > self.best_width && !is_space(next) {
                    self.write_indent()?;
                } else {
                    self.write_char(ch)?;
                }
                spaces = true;
            } else if is_break(ch) {
                if !breaks && ch == '\n' {
                    self.put_break()?;
                }
                self.write_break(ch)?;
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent()?;
                }
                self.write_char(ch)?;
                self.indention = false;
                spaces = false;
                breaks = false;
            }
        }
        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_single_quoted_scalar(&mut self, value: &str, allow_breaks: bool) -> Result<()> {
        let mut spaces = false;
        let mut breaks = false;
        self.write_indicator("'", true, false, false)?;
        let mut chars = value.chars();
        let mut is_first = true;
        while let Some(ch) = chars.next() {
            let next = chars.clone().next();
            let is_last = next.is_none();

            if is_space(ch) {
                if allow_breaks
                    && !spaces
                    && self.column > self.best_width
                    && !is_first
                    && !is_last
                    && !is_space(next)
                {
                    self.write_indent()?;
                } else {
                    self.write_char(ch)?;
                }
                spaces = true;
            } else if is_break(ch) {
                if !breaks && ch == '\n' {
                    self.put_break()?;
                }
                self.write_break(ch)?;
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent()?;
                }
                if ch == '\'' {
                    self.put('\'')?;
                }
                self.write_char(ch)?;
                self.indention = false;
                spaces = false;
                breaks = false;
            }

            is_first = false;
        }
        if breaks {
            self.write_indent()?;
        }
        self.write_indicator("'", false, false, false)?;
        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_double_quoted_scalar(&mut self, value: &str, allow_breaks: bool) -> Result<()> {
        let mut spaces = false;
        self.write_indicator("\"", true, false, false)?;
        let mut chars = value.chars();
        let mut first = true;
        while let Some(ch) = chars.next() {
            if !is_printable(ch)
                || !self.unicode && !is_ascii(ch)
                || is_bom(ch)
                || is_break(ch)
                || ch == '"'
                || ch == '\\'
            {
                self.put('\\')?;
                match ch {
                    // TODO: Double check these character mappings.
                    '\0' => {
                        self.put('0')?;
                    }
                    '\x07' => {
                        self.put('a')?;
                    }
                    '\x08' => {
                        self.put('b')?;
                    }
                    '\x09' => {
                        self.put('t')?;
                    }
                    '\x0A' => {
                        self.put('n')?;
                    }
                    '\x0B' => {
                        self.put('v')?;
                    }
                    '\x0C' => {
                        self.put('f')?;
                    }
                    '\x0D' => {
                        self.put('r')?;
                    }
                    '\x1B' => {
                        self.put('e')?;
                    }
                    '\x22' => {
                        self.put('"')?;
                    }
                    '\x5C' => {
                        self.put('\\')?;
                    }
                    '\u{0085}' => {
                        self.put('N')?;
                    }
                    '\u{00A0}' => {
                        self.put('_')?;
                    }
                    '\u{2028}' => {
                        self.put('L')?;
                    }
                    '\u{2029}' => {
                        self.put('P')?;
                    }
                    _ => {
                        let (prefix, width) = if ch <= '\u{00ff}' {
                            ('x', 2)
                        } else if ch <= '\u{ffff}' {
                            ('u', 4)
                        } else {
                            ('U', 8)
                        };
                        self.put(prefix)?;
                        let mut k = (width - 1) * 4;
                        let value_0 = ch as u32;
                        while k >= 0 {
                            let digit = (value_0 >> k) & 0x0F;
                            let digit_char = match char::from_digit(digit, 16) {
                                Some(digit_char) => digit_char,
                                None => unreachable!("digit out of range"),
                            };
                            // The libyaml emitter encodes unicode sequences as uppercase hex.
                            let digit_char = digit_char.to_ascii_uppercase();
                            self.put(digit_char)?;
                            k -= 4;
                        }
                    }
                }
                spaces = false;
            } else if is_space(ch) {
                if allow_breaks
                    && !spaces
                    && self.column > self.best_width
                    && !first
                    && chars.clone().next().is_some()
                {
                    self.write_indent()?;
                    if is_space(chars.clone().next()) {
                        self.put('\\')?;
                    }
                } else {
                    self.write_char(ch)?;
                }
                spaces = true;
            } else {
                self.write_char(ch)?;
                spaces = false;
            }

            first = false;
        }
        self.write_indicator("\"", false, false, false)?;
        self.whitespace = false;
        self.indention = false;
        Ok(())
    }

    fn write_block_scalar_hints(&mut self, string: &str) -> Result<()> {
        let mut chomp_hint: Option<&str> = None;

        let first = string.chars().next();
        if is_space(first) || is_break(first) {
            let indent_hint = match char::from_digit(self.best_indent as u32, 10) {
                Some(indent_hint) => indent_hint,
                None => unreachable!("self.best_indent out of range"),
            };
            let mut indent_hint_buffer = [0u8; 1];
            let indent_hint = indent_hint.encode_utf8(&mut indent_hint_buffer);
            self.write_indicator(indent_hint, false, false, false)?;
        }
        self.open_ended = 0;

        if string.is_empty() {
            chomp_hint = Some("-");
        } else {
            let mut chars_rev = string.chars().rev();
            let ch = chars_rev.next();
            let next = chars_rev.next();

            if !is_break(ch) {
                chomp_hint = Some("-");
            } else if is_breakz(next) {
                chomp_hint = Some("+");
                self.open_ended = 2;
            }
        }

        if let Some(chomp_hint) = chomp_hint {
            self.write_indicator(chomp_hint, false, false, false)?;
        }
        Ok(())
    }

    fn write_literal_scalar(&mut self, value: &str) -> Result<()> {
        let mut breaks = true;
        self.write_indicator("|", true, false, false)?;
        self.write_block_scalar_hints(value)?;
        self.put_break()?;
        self.indention = true;
        self.whitespace = true;
        let chars = value.chars();
        for ch in chars {
            if is_break(ch) {
                self.write_break(ch)?;
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent()?;
                }
                self.write_char(ch)?;
                self.indention = false;
                breaks = false;
            }
        }
        Ok(())
    }

    fn write_folded_scalar(&mut self, value: &str) -> Result<()> {
        let mut breaks = true;
        let mut leading_spaces = true;
        self.write_indicator(">", true, false, false)?;
        self.write_block_scalar_hints(value)?;
        self.put_break()?;
        self.indention = true;
        self.whitespace = true;

        let mut chars = value.chars();

        while let Some(ch) = chars.next() {
            if is_break(ch) {
                if !breaks && !leading_spaces && ch == '\n' {
                    let mut skip_breaks = chars.clone();
                    while is_break(skip_breaks.next()) {}
                    if !is_blankz(skip_breaks.next()) {
                        self.put_break()?;
                    }
                }
                self.write_break(ch)?;
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent()?;
                    leading_spaces = is_blank(ch);
                }
                if !breaks
                    && is_space(ch)
                    && !is_space(chars.clone().next())
                    && self.column > self.best_width
                {
                    self.write_indent()?;
                } else {
                    self.write_char(ch)?;
                }
                self.indention = false;
                breaks = false;
            }
        }
        Ok(())
    }

    /// Flush the accumulated characters to the output.
    pub fn flush(&mut self) -> Result<()> {
        assert!((self.write_handler).is_some());
        assert_ne!(self.encoding, Encoding::Any);

        if self.buffer.is_empty() {
            return Ok(());
        }

        if self.encoding == Encoding::Utf8 {
            let to_emit = self.buffer.as_bytes();
            self.write_handler
                .as_mut()
                .expect("non-null writer")
                .write_all(to_emit)?;
            self.buffer.clear();
            return Ok(());
        }

        let big_endian = match self.encoding {
            Encoding::Any | Encoding::Utf8 => {
                unreachable!("unhandled encoding")
            }
            Encoding::Utf16Le => false,
            Encoding::Utf16Be => true,
        };

        for ch in self.buffer.encode_utf16() {
            let bytes = if big_endian {
                ch.to_be_bytes()
            } else {
                ch.to_le_bytes()
            };
            self.raw_buffer.extend(bytes);
        }

        let to_emit = self.raw_buffer.as_slice();

        self.write_handler
            .as_mut()
            .expect("non-null function pointer")
            .write_all(to_emit)?;
        self.buffer.clear();
        self.raw_buffer.clear();
        Ok(())
    }

    pub(crate) fn reset_anchors(&mut self) {
        self.anchors.clear();
        self.last_anchor_id = 0;
    }

    pub(crate) fn anchor_node_sub(&mut self, index: i32) {
        self.anchors[index as usize - 1].references += 1;
        if self.anchors[index as usize - 1].references == 2 {
            self.last_anchor_id += 1;
            self.anchors[index as usize - 1].anchor = self.last_anchor_id;
        }
    }

    pub(crate) fn generate_anchor(anchor_id: i32) -> String {
        alloc::format!("id{anchor_id:03}")
    }
}
