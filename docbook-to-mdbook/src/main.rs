#![expect(clippy::single_char_add_str)]

use {
    anyhow::{Context, bail},
    bstr::ByteSlice,
    isnt::std_1::primitive::IsntStrExt,
    quick_xml::{
        Reader,
        events::{BytesStart, Event},
    },
    regex::Regex,
    std::{
        borrow::Cow,
        fs::{File, read_dir},
        io::{BufWriter, Write},
        mem,
        path::Path,
        sync::LazyLock,
    },
    textwrap::{Options, WordSeparator, WordSplitter, WrapAlgorithm},
};

const XML_ROOT: &str = "../doc/publican";
const MD_ROOT: &str = "../doc/book";

fn main() -> anyhow::Result<()> {
    for file in read_dir(XML_ROOT).context("open publican")? {
        let file = file.unwrap();
        let path = file.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.to_str().unwrap();
        if !name.ends_with(".xml") {
            continue;
        }
        match name {
            "Architecture.xml"
            | "Color.xml"
            | "Compositors.xml"
            | "Content_Updates.xml"
            | "Introduction.xml"
            | "Message_XML.xml"
            | "Protocol.xml"
            | "Foreword.xml"
            | "Xwayland.xml" => handle_chapter_file(name)
                .with_context(|| format!("handle_chapter_file {}", name))?,
            "Book_Info.xml" | "Wayland.xml" | "Author_Group.xml" | "Client.xml" | "Server.xml" => {
                continue;
            }
            _ => bail!("unknown file name {name}"),
        }
    }
    handle_wayland_xml().context("Wayland.xml")?;
    Ok(())
}

fn handle_wayland_xml() -> anyhow::Result<()> {
    let input = Path::new(XML_ROOT).join("Wayland.xml");
    let input = std::fs::read_to_string(input).context("read xml")?;
    let mut r = Reader::from_str(&input);
    let output = Path::new(MD_ROOT).join("src").join("SUMMARY.md");
    let mut w = BufWriter::new(File::create(output).context("open md")?);
    writeln!(w, "# Summary")?;
    writeln!(w)?;
    writeln!(w, "[Foreword](Foreword.md)")?;
    writeln!(w)?;
    loop {
        let event = r.read_event().context("read event")?;
        match event {
            Event::Start(c) => match c.name().as_ref() {
                b"book" => loop {
                    let event = r.read_event().context("read event")?;
                    match event {
                        Event::Empty(e) if e.name().as_ref() == b"xi:include" => {
                            let href = e
                                .try_get_attribute("href")
                                .context("href")?
                                .context("href")?
                                .unescape_value()
                                .context("href")?;
                            let name = match &*href {
                                "Book_Info.xml" => continue,
                                "Foreword.xml" => continue,
                                "Introduction.xml" => "Introduction",
                                "Compositors.xml" => "Types of Compositors",
                                "Architecture.xml" => "Wayland Architecture",
                                "Protocol.xml" => "Wayland Protocol and Model of Operation",
                                "Message_XML.xml" => "Message Definition Language",
                                "Xwayland.xml" => "X11 Application Support",
                                "Content_Updates.xml" => "Content Updates",
                                "Color.xml" => "Color management",
                                "ProtocolSpec.xml" => continue,
                                "Client.xml" => continue,
                                "Server.xml" => continue,
                                _ => bail!("unexpected link {href}"),
                            };
                            writeln!(w, "- [{name}](./{})", href.replace(".xml", ".md"))?;
                        }
                        Event::Text(t) if t.trim().is_empty() => {}
                        Event::End(e) if e.name().as_ref() == b"book" => break,
                        _ => bail!("unexpected event {event:?}"),
                    }
                },
                s => bail!("unexpected start {:?}", s.as_bstr()),
            },
            Event::Decl(_) => {}
            Event::DocType(_) => {}
            Event::Text(t) if t.trim().is_empty() => {}
            Event::Eof => break,
            _ => bail!("unexpected event {event:?}"),
        }
    }
    Ok(())
}

fn handle_chapter_file(name: &str) -> anyhow::Result<()> {
    let input = Path::new(XML_ROOT).join(name);
    let input = std::fs::read_to_string(input).context("read xml")?;
    let mut reader = Reader::from_str(&input);
    let output = Path::new(MD_ROOT)
        .join("src")
        .join(name)
        .with_extension("md");
    let mut output = BufWriter::new(File::create(output).context("open md")?);
    Handler {
        w: &mut output,
        r: &mut reader,
        text: Default::default(),
        need_newline: false,
        last_line_has_content: false,
        have_indent_1: Default::default(),
        indent_1: Default::default(),
        indent_2: Default::default(),
    }
    .handle_chapter_file()
}

struct Handler<'a, 'b, W> {
    w: &'a mut W,
    r: &'a mut Reader<&'b [u8]>,
    text: String,
    need_newline: bool,
    last_line_has_content: bool,
    have_indent_1: bool,
    indent_1: String,
    indent_2: String,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum TitleType {
    Chapter,
    Section(usize),
    Bold,
}

enum ListType {
    Bullet,
    Numbered(usize),
    Itemized,
}

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\n\t ]+"#).unwrap());

impl<W> Handler<'_, '_, W>
where
    W: Write,
{
    fn handle_chapter_file(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            match event {
                Event::Start(c) => match c.name().as_ref() {
                    b"preface" | b"chapter" => self.handle_chapter().context("handle_chapter")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::Decl(_) => {}
                Event::DocType(_) => {}
                Event::Text(t) if t.trim().is_empty() => {}
                Event::Eof => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_chapter(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(s) => match s.name().as_ref() {
                    b"title" => self
                        .handle_title(TitleType::Chapter)
                        .context("handle_title")?,
                    b"literallayout" | b"para" => self.handle_para(false).context("handle_para")?,
                    b"section" => self.handle_section(0).context("handle_section")?,
                    _ => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::Empty(s) if s.name().as_ref() == b"xi:include" => {}
                Event::End(e) if e.name().as_ref() == b"chapter" => break,
                Event::End(e) if e.name().as_ref() == b"preface" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_title(&mut self, ty: TitleType) -> anyhow::Result<()> {
        match ty {
            TitleType::Chapter => self.text.push_str("# "),
            TitleType::Section(depth) => {
                self.text.push_str("##");
                for _ in 0..depth {
                    self.text.push_str("#");
                }
                self.text.push_str(" ");
            }
            TitleType::Bold => self.text.push_str("**"),
        }
        let mut first = true;
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Text(v) => {
                    if first {
                        first = false;
                    } else {
                        self.text.push_str(" ");
                    }
                    self.text.push_str(v.decode().unwrap().trim());
                }
                Event::End(v) if &*v == b"title" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        match ty {
            TitleType::Chapter => {}
            TitleType::Section(_) => {}
            TitleType::Bold => self.text.push_str("**"),
        }
        let text = mem::take(&mut self.text);
        self.write_line(&text)?;
        self.text = text;
        self.text.clear();
        self.need_newline = true;
        Ok(())
    }

    fn handle_section(&mut self, depth: usize) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(s) => match s.name().as_ref() {
                    b"section" => self.handle_section(depth + 1).context("handle_section")?,
                    b"title" => self
                        .handle_title(TitleType::Section(depth))
                        .context("handle_title")?,
                    b"synopsis" => self.handle_para(true).context("handle_synopsis")?,
                    b"para" => self.handle_para(false).context("handle_para")?,
                    b"orderedlist" => self.handle_orderedlist().context("handle_orderedlist")?,
                    b"itemizedlist" => self.handle_itemizedlist().context("handle_itemizedlist")?,
                    b"figure" => self.handle_figure().context("handle_figure")?,
                    b"variablelist" => self.handle_variablelist().context("handle_variablelist")?,
                    b"mediaobject" | b"mediaobjectco" => {
                        self.handle_mediaobject().context("handle_mediaobject")?
                    }
                    _ => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"section" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_xref(&mut self, e: BytesStart<'_>) -> anyhow::Result<()> {
        let url = e
            .try_get_attribute("linkend")
            .context("linkend")?
            .context("linkend")?
            .unescape_value()
            .context("linkend")?;
        let name_buf;
        let url_buf;
        let (name, url) = if let Some(v) = url.strip_prefix("sect-")
            && let Some((sect, v)) = v.split_once("-")
        {
            match sect {
                "Protocol" => match v {
                    "Wire-Format" => ("Wire Format", "#wire-format"),
                    "data-sharing-devices" => ("Data devices", "#data-devices"),
                    _ => bail!("unhandled protocol link {url}"),
                },
                "MessageXML" => match v {
                    "tag-interface" => ("interface", "#interface"),
                    "tag-arg" => ("arg", "#arg"),
                    _ => bail!("unhandled message-xml link {url}"),
                },
                _ => bail!("unhandled section {url}"),
            }
        } else if let Some(v) = url.strip_prefix("chap-") {
            match v {
                "Protocol" => ("Wayland Protocol and Model of Operation", "Protocol.md"),
                _ => bail!("unhandled chap {url}"),
            }
        } else if let Some(v) = url.strip_prefix("protocol-spec-") {
            if let Some((interface, v)) = v.split_once("-")
                && let Some((ty, message)) = v.split_once("-")
            {
                name_buf = format!("{interface}.{message}");
                url_buf =
                    format!("https://wayland.app/protocols/wayland#{interface}:{ty}:{message}");
            } else {
                name_buf = v.to_string();
                url_buf = format!("https://wayland.app/protocols/wayland#{v}");
            }
            (&*name_buf, &*url_buf)
        } else {
            bail!("unknown link format {url}");
        };
        self.text.push_str("[");
        self.text.push_str(name);
        self.text.push_str("]");
        self.text.push_str("(");
        self.text.push_str(url);
        self.text.push_str(")");
        Ok(())
    }

    fn handle_filename(&mut self) -> anyhow::Result<()> {
        let mut text = String::new();
        loop {
            let event = self.r.read_event().context("read event")?;
            match event {
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    text.push_str(&s);
                }
                Event::End(v) if &*v == b"filename" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        self.text.push_str("[");
        self.text.push_str(text.trim());
        self.text.push_str("]");
        self.text
            .push_str("(https://gitlab.freedesktop.org/wayland/wayland/-/blob/main/");
        self.text.push_str(&text);
        self.text.push_str(")");
        Ok(())
    }

    fn handle_link(&mut self, e: BytesStart<'_>) -> anyhow::Result<()> {
        let mut text = String::new();
        loop {
            let event = self.r.read_event().context("read event")?;
            match event {
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    text.push_str(&s);
                }
                Event::End(v) if &*v == b"link" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        let url = e
            .try_get_attribute("linkend")
            .context("linkend")?
            .context("linkend")?
            .unescape_value()
            .context("linkend")?;
        let buf;
        let url = if let Some(v) = url.strip_prefix("sect-")
            && let Some((sect, v)) = v.split_once("-")
        {
            match sect {
                "Compositors" => match v {
                    "System-Compositor" => "#system-compositor",
                    "Session-Compositor" => "#session-compositor",
                    _ => bail!("unhandled compositors link {url}"),
                },
                "Library" => "https://gitlab.freedesktop.org/wayland/wayland",
                _ => bail!("unhandled section {url}"),
            }
        } else if let Some(v) = url.strip_prefix("protocol-spec-") {
            if let Some((interface, v)) = v.split_once("-")
                && let Some((ty, message)) = v.split_once("-")
            {
                buf = format!("https://wayland.app/protocols/wayland#{interface}:{ty}:{message}");
            } else {
                buf = format!("https://wayland.app/protocols/wayland#{v}");
            }
            &buf
        } else {
            bail!("unknown link format {url}");
        };
        self.text.push_str("[");
        self.text.push_str(text.trim());
        self.text.push_str("]");
        self.text.push_str("(");
        self.text.push_str(url);
        self.text.push_str(")");
        Ok(())
    }

    fn handle_ulink(&mut self, e: BytesStart<'_>) -> anyhow::Result<()> {
        let mut text = String::new();
        loop {
            let event = self.r.read_event().context("read event")?;
            match event {
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    text.push_str(&s);
                }
                Event::End(v) if &*v == b"ulink" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        let url = e
            .try_get_attribute("url")
            .context("url")?
            .context("url")?
            .unescape_value()
            .context("url")?;
        self.text.push_str("[");
        self.text.push_str(text.trim());
        self.text.push_str("]");
        self.text.push_str("(");
        self.text.push_str(&url);
        self.text.push_str(")");
        Ok(())
    }

    fn handle_code(&mut self) -> anyhow::Result<()> {
        self.text.push_str("`");
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    self.text.push_str(&s);
                }
                Event::End(v) if &*v == b"code" => break,
                Event::End(v) if &*v == b"varname" => break,
                Event::End(v) if &*v == b"literal" => break,
                Event::End(v) if &*v == b"userinput" => break,
                Event::End(v) if &*v == b"type" => break,
                Event::End(v) if &*v == b"function" => break,
                Event::End(v) if &*v == b"systemitem" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        self.text.push_str("`");
        Ok(())
    }

    fn handle_emphasis(&mut self) -> anyhow::Result<()> {
        self.text.push_str("_");
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    self.text.push_str(&s);
                }
                Event::End(v) if &*v == b"emphasis" => break,
                Event::End(v) if &*v == b"firstterm" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        self.text.push_str("_");
        Ok(())
    }

    fn handle_newline(&mut self) -> anyhow::Result<()> {
        if mem::take(&mut self.need_newline) {
            self.write_line("")?;
        }
        Ok(())
    }

    fn write_line(&mut self, text: &str) -> anyhow::Result<()> {
        self.handle_newline()?;
        let text = text.trim_end();
        if text.is_empty() {
            let indent = self.indent_1.trim_end();
            if indent.is_empty() {
                if mem::take(&mut self.last_line_has_content) {
                    writeln!(self.w)?;
                    // dbg!("<NL>");
                }
            } else {
                writeln!(self.w, "{}", indent)?;
                // dbg!("indent");
                self.last_line_has_content = true;
            }
        } else {
            writeln!(self.w, "{}{text}", self.indent_1)?;
            // dbg!(format!("{}{text}", self.indent_1));
            self.last_line_has_content = true;
        }
        self.clear_indent_1();
        Ok(())
    }

    fn clear_indent_1(&mut self) {
        if self.have_indent_1 {
            self.have_indent_1 = false;
            self.indent_1.clone_from(&self.indent_2);
        }
    }

    fn push_indent(
        &mut self,
        indent: &str,
        cont: bool,
        f: impl FnOnce(&mut Self) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.handle_newline()?;
        let len = self.indent_1.len();
        self.have_indent_1 = true;
        self.indent_1.push_str(indent);
        self.indent_1.push_str(" ");
        if cont {
            self.indent_2.push_str(indent);
            self.indent_2.push_str(" ");
        } else {
            for _ in self.indent_2.len()..self.indent_1.len() {
                self.indent_2.push_str(" ");
            }
        }
        f(self)?;
        self.indent_1.truncate(len);
        self.indent_2.truncate(len);
        Ok(())
    }

    fn flush_text(&mut self, synopsis: bool) -> anyhow::Result<()> {
        const MAX_WIDTH: usize = 80;
        self.handle_newline()?;
        if synopsis {
            self.write_line("```")?;
        }
        let text = mem::take(&mut self.text);
        let trimmed = text.trim();
        if trimmed.is_not_empty() {
            let options = Options::new(MAX_WIDTH)
                .break_words(false)
                .wrap_algorithm(WrapAlgorithm::FirstFit)
                .word_separator(WordSeparator::AsciiSpace)
                .word_splitter(WordSplitter::NoHyphenation)
                .initial_indent(&self.indent_1)
                .subsequent_indent(&self.indent_2);
            let lines = textwrap::wrap(trimmed.trim(), options);
            for line in lines {
                writeln!(self.w, "{line}")?;
                // dbg!(line);
                self.last_line_has_content = true;
                self.clear_indent_1();
            }
            self.text = text;
        }
        if synopsis {
            self.write_line("```")?;
        }
        self.text.clear();
        Ok(())
    }

    fn handle_admonition(&mut self, name: &str) -> anyhow::Result<()> {
        self.push_indent(">", true, |slf| {
            slf.write_line(&format!("[!{name}]"))?;
            loop {
                let event = slf.r.read_event().context("read event")?;
                // dbg!(&event);
                match event {
                    Event::Start(v) => match v.name().as_ref() {
                        b"simpara" | b"para" => slf.handle_para(false).context("handle_para")?,
                        s => bail!("unexpected start {:?}", s.as_bstr()),
                    },
                    Event::Text(t) if t.trim().is_empty() => {}
                    Event::End(v) if &*v == b"warning" => break,
                    Event::End(v) if &*v == b"note" => break,
                    _ => bail!("unexpected event {event:?}"),
                }
            }
            Ok(())
        })
    }

    fn handle_para(&mut self, synopsis: bool) -> anyhow::Result<()> {
        self.handle_text_block(synopsis)?;
        self.need_newline = true;
        Ok(())
    }

    fn handle_text_block(&mut self, synopsis: bool) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(v) => match v.name().as_ref() {
                    b"synopsis" => {
                        self.flush_text(synopsis)?;
                        self.need_newline = true;
                        self.handle_para(true).context("handle_synopsis")?
                    }
                    b"emphasis" => self.handle_emphasis().context("handle_emphasis")?,
                    b"firstterm" => self.handle_emphasis().context("handle_firstterm")?,
                    b"type" => self.handle_code().context("handle_type")?,
                    b"code" => self.handle_code().context("handle_code")?,
                    b"systemitem" => self.handle_code().context("handle_systemitem")?,
                    b"function" => self.handle_code().context("handle_function")?,
                    b"filename" => self.handle_filename().context("handle_filename")?,
                    b"literal" => self.handle_code().context("handle_literal")?,
                    b"varname" => self.handle_code().context("handle_varname")?,
                    b"userinput" => self.handle_code().context("handle_userinput")?,
                    b"ulink" => self.handle_ulink(v).context("handle_ulink")?,
                    b"link" => self.handle_link(v).context("handle_link")?,
                    b"itemizedlist" => {
                        self.flush_text(synopsis)?;
                        self.need_newline = true;
                        self.handle_itemizedlist().context("handle_itemizedlist")?
                    }
                    b"orderedlist" => {
                        self.flush_text(synopsis)?;
                        self.need_newline = true;
                        self.handle_orderedlist().context("handle_orderedlist")?
                    }
                    b"variablelist" => {
                        self.flush_text(synopsis)?;
                        self.need_newline = true;
                        self.handle_variablelist().context("handle_variablelist")?
                    }
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::Empty(v) => match v.name().as_ref() {
                    b"xref" => self.handle_xref(v).context("handle_xref")?,
                    e => bail!("unexpected empty {:?}", e.as_bstr()),
                },
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    self.text.push_str(&s);
                }
                Event::GeneralRef(v) => {
                    self.text.push_str(&v.decode().unwrap());
                }
                Event::End(v) if &*v == b"para" => break,
                Event::End(v) if &*v == b"synopsis" => break,
                Event::End(v) if &*v == b"simpara" => break,
                Event::End(v) if &*v == b"literallayout" => break,
                _ => bail!("unexpected event {event:?}"),
            }
        }
        self.flush_text(synopsis)?;
        Ok(())
    }

    fn handle_variablelist(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"title" => self.handle_title(TitleType::Bold).context("handle_title")?,
                    b"varlistentry" => self.handle_varlistentry().context("handle_varlistentry")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"variablelist" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_varlistentry(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"term" => self.handle_term().context("handle_term")?,
                    b"listitem" => self
                        .handle_listitem(ListType::Itemized)
                        .context("handle_listitem")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"varlistentry" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_term(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"literal" => self.handle_code().context("handle_literal")?,
                    b"synopsis" => self.handle_text_block(false).context("handle_synopsis")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"term" => break,
                Event::Text(t) => {
                    let s = t.decode().unwrap();
                    let s = WHITESPACE.replace_all(&s, " ");
                    self.text.push_str(&s);
                }
                _ => bail!("unexpected event {event:?}"),
            }
        }
        self.flush_text(false)?;
        Ok(())
    }

    fn handle_orderedlist(&mut self) -> anyhow::Result<()> {
        let mut n = 1..;
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"listitem" => self
                        .handle_listitem(ListType::Numbered(n.next().unwrap()))
                        .context("handle_listitem")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"orderedlist" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_itemizedlist(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"listitem" => self
                        .handle_listitem(ListType::Bullet)
                        .context("handle_listitem")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"itemizedlist" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_listitem(&mut self, ty: ListType) -> anyhow::Result<()> {
        let indent = match ty {
            ListType::Bullet => Cow::Borrowed("-"),
            ListType::Numbered(n) => Cow::Owned(format!("{n}.")),
            ListType::Itemized => Cow::Borrowed("  :"),
        };
        self.push_indent(&indent, false, |slf| {
            loop {
                let event = slf.r.read_event().context("read event")?;
                // dbg!(&event);
                match event {
                    Event::Start(e) => match e.name().as_ref() {
                        b"simpara" | b"para" => slf.handle_para(false).context("handle_para")?,
                        b"warning" => slf.handle_admonition("WARNING").context("handle_warning")?,
                        b"note" => slf.handle_admonition("NOTE").context("handle_note")?,
                        b"variablelist" => {
                            slf.handle_variablelist().context("handle_variablelist")?
                        }
                        s => bail!("unexpected start {:?}", s.as_bstr()),
                    },
                    Event::End(v) if &*v == b"listitem" => break,
                    Event::Text(t) if t.trim().is_empty() => {}
                    _ => bail!("unexpected event {event:?}"),
                }
            }
            Ok(())
        })
    }

    fn handle_figure(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"title" => self.handle_title(TitleType::Bold).context("handle_title")?,
                    b"mediaobject" | b"mediaobjectco" => {
                        self.handle_mediaobject().context("handle_mediaobject")?
                    }
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"figure" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_mediaobject(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"textobject" => self.handle_textobject().context("handle_textobject")?,
                    b"imageobject" => self.handle_imageobject().context("handle_imageobject")?,
                    b"imageobjectco" => self
                        .handle_imageobjectco()
                        .context("handle_imageobjectco")?,
                    b"caption" => self.handle_caption().context("handle_caption")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"mediaobject" => break,
                Event::End(v) if &*v == b"mediaobjectco" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_textobject(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::End(v) if &*v == b"textobject" => break,
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_imageobject(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"areaspec" => self.handle_areaspec().context("handle_areaspec")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::Empty(e) => match e.name().as_ref() {
                    b"imagedata" => self.handle_imagedata(e).context("handle_imagedata")?,
                    s => bail!("unexpected empty {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"imageobject" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_imageobjectco(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"areaspec" => self.handle_areaspec().context("handle_areaspec")?,
                    b"imageobject" => self.handle_imageobject().context("handle_imageobject")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"imageobjectco" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }

    fn handle_areaspec(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::End(v) if &*v == b"areaspec" => break,
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_imagedata(&mut self, e: BytesStart<'_>) -> anyhow::Result<()> {
        let fileref = e
            .try_get_attribute("fileref")
            .context("fileref")?
            .context("fileref")?
            .unescape_value()
            .unwrap();
        self.write_line(&format!("![]({fileref})"))?;
        self.need_newline = true;
        Ok(())
    }

    fn handle_caption(&mut self) -> anyhow::Result<()> {
        loop {
            let event = self.r.read_event().context("read event")?;
            // dbg!(&event);
            match event {
                Event::Start(e) => match e.name().as_ref() {
                    b"para" => self.handle_para(false).context("handle_para")?,
                    s => bail!("unexpected start {:?}", s.as_bstr()),
                },
                Event::End(v) if &*v == b"caption" => break,
                Event::Text(t) if t.trim().is_empty() => {}
                _ => bail!("unexpected event {event:?}"),
            }
        }
        Ok(())
    }
}
