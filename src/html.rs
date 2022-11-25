use std::{
    collections::BTreeMap,
    fmt::Display,
    option::Option,
    str::{pattern::Pattern, FromStr},
};

use anyhow::anyhow;
use nom::{
    branch::alt,
    bytes::complete::{is_not, tag, take_until},
    character::complete::{alpha1, alphanumeric1, char, multispace0, multispace1},
    combinator::{map, opt, recognize, verify},
    error::{context, ContextError, ParseError, VerboseError},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded, terminated},
    IResult,
};

#[derive(Debug, Clone)]
pub struct Element {
    name: String,
    attrs: BTreeMap<String, String>,
    contents: Vec<Content>,
}
impl Element {
    pub fn get_attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(|s| s.as_str())
    }
    pub fn has_attr<'k, P: Pattern<'k> + Copy>(&'k self, key: P, value: Option<P>) -> bool {
        match value {
            Some(v_pat) => {
                self.attrs.iter().any(|(k, v)| k.find(key).is_some() & v.find(v_pat).is_some())
            }
            None => self.attrs.iter().any(|(k, v)| k.find(key).is_some() & v.is_empty()),
        }
    }
    pub fn iter_children(&self) -> impl Iterator<Item = &Element> {
        self.contents.iter().filter_map(|c| match c {
            Content::Text(_) => None,
            Content::Element(elem) => Some(elem),
        })
    }
    pub fn iter_decendents<'s>(&'s self) -> Box<dyn Iterator<Item = &Element> + 's> {
        Box::new(
            self.iter_children()
                .map(|elem| Some(elem).into_iter().chain(elem.iter_decendents()))
                .flatten(),
        )
    }
    pub fn iter_contents(&self) -> impl Iterator<Item = &Content> {
        self.contents.iter()
    }
}
impl Display for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Element").field("name", &self.name).field("attrs", &self.attrs).finish()?;
        for content in &self.contents {
            let disp = format!("{}", content);
            let lines = disp.lines();
            for line in lines {
                f.write_str("\n|   ")?;
                f.write_str(line)?;
            }
        }
        Ok(())
    }
}
impl FromStr for Element {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match xml_element::<VerboseError<&str>>(s.trim()) {
            Ok((_, elem)) => Ok(elem),
            Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(anyhow!(e.to_string())),
            _ => unimplemented!(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    Element(Element),
}
impl Content {
    pub fn as_text(self) -> Option<String> {
        match self {
            Content::Text(t) => Some(t),
            Content::Element(_) => None,
        }
    }
    pub fn as_element(self) -> Option<Element> {
        match self {
            Content::Text(_) => None,
            Content::Element(elem) => Some(elem),
        }
    }
}
impl Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Content::Text(s) => s.fmt(f),
            Content::Element(e) => e.fmt(f),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Tag {
    name: String,
    attrs: BTreeMap<String, String>,
    is_close: bool,
}

fn xml_element<'i, E>(i: &'i str) -> IResult<&str, Element, E>
where
    E: ContextError<&'i str> + ParseError<&'i str>,
{
    let (i, first_tag) = verify(xml_tag, |tag| !tag.is_close)(i)?;
    let name = first_tag.name;
    let attrs = first_tag.attrs;
    let i_before = i;
    let close_p = verify(preceded(multispace0, xml_tag), |tag| tag.is_close && tag.name == name);
    let (i_after, maybe_contents) = opt(terminated(many0(xml_content), close_p))(i)?;
    let (i, contents) = match maybe_contents {
        Some(contents) => (i_after, contents),
        None => (i_before, vec![]),
    };
    Ok((i, Element { name, attrs, contents }))
}

fn xml_content<'i, E>(i: &'i str) -> IResult<&str, Content, E>
where
    E: ContextError<&'i str> + ParseError<&'i str>,
{
    let to_trim_string = |s: &str| s.trim().to_string();
    let not_empty = |s: &str| !s.is_empty();
    let trim_text_p = verify(map(is_not("<"), to_trim_string), not_empty);
    let element_p = delimited(multispace0, xml_element, multispace0);
    let content_p = alt((map(element_p, Content::Element), map(trim_text_p, Content::Text)));
    context("Failed to parse XML content", content_p)(i)
}

fn xml_tag<'i, E: ParseError<&'i str>>(i: &'i str) -> IResult<&str, Tag, E> {
    let (i, _) = char('<')(i)?;
    let (i, start_slash) = opt(char('/'))(i)?;
    let (i, name) = map(xml_name, |s| s.to_string())(i)?;
    let attrs_p = preceded(multispace1, separated_list0(multispace1, xml_attr));
    let (i, attrs_vec) = map(opt(attrs_p), Option::unwrap_or_default)(i)?;
    let (i, _) = multispace0(i)?;
    let attrs = attrs_vec.into_iter().collect();
    let (i, _) = opt(char('/'))(i)?;
    let (i, _) = char('>')(i)?;
    Ok((i, Tag { name, attrs, is_close: start_slash.is_some() }))
}

fn xml_attr<'i, E: ParseError<&'i str>>(i: &'i str) -> IResult<&str, (String, String), E> {
    let (i, name) = xml_name(i)?;
    let value_p = delimited(tag("=\""), take_until("\""), tag("\""));
    let (i, value) = map(opt(value_p), Option::unwrap_or_default)(i)?;
    Ok((i, (name.to_string(), value.to_string())))
}

fn xml_name<'i, E: ParseError<&'i str>>(i: &'i str) -> IResult<&str, &str, E> {
    let start_p = alt((alpha1, tag("_")));
    let rest_p = alt((alphanumeric1, tag("-"), tag("_"), tag(".")));
    recognize(pair(start_p, many0(rest_p)))(i)
}

#[cfg(test)]
mod tests {
    use std::mem::size_of_val;

    use itertools::Itertools;
    use nom::error::VerboseError;

    use super::*;

    #[test]
    fn test_parse_xml() {
        let start = std::time::Instant::now();
        let (rest, elem) = xml_element::<VerboseError<&str>>(TEST2).unwrap();
        let end = std::time::Instant::now();
        // println!("Rest: `{rest}`");
        println!("{elem}");
        println!("{:?}", end - start);
        let size = elem.iter_decendents().map(|elem| size_of_val(elem)).sum::<usize>();
        println!("Size: {size}");
        // let pretty = format!("{elem}");
        // assert_eq!(rest, "");
        // assert_eq!(pretty, RESULT1);
    }

    #[test]
    fn test_iter_decendents() {
        let (_, root) = xml_element::<VerboseError<&str>>(TEST1).unwrap();
        let children = root.iter_decendents().map(|elem| (&elem.name, &elem.attrs)).collect_vec();
        for child in children {
            println!("{child:?}");
        }
    }

    const TEST1: &str = "<div class=\"row\">\n\t<div class=\"columns\">\n\t\t<div class=\"row\"><div class=\"columns\">\n\t \n\t <form data-abide id=\"frm-update-date-range\" name=\"frm-update-date-range\" class=\"custom\" target=\"#ajax-booking-view-tool-1669869926\" action=\"ajax.get-bookings.php\">\n\t \t\n\t \t<div class=\"row \">\n            <div class=\"columns\">\n                <label>Select Tool(s)</label>\n                <input type=\"hidden\" name=\"tool_id[]\" multiple=\"multiple\" class=\"select2-ajax\" source=\"ajax.get-tools.php\" hide_inactive=\"1\" data-placeholder=\"Select Tools.. (leave blank for all tools)\" value=\"\"/>\n                <small class=\"error\">Pick some tools</small> \n            </div>\n        </div>\n        \n        <div class=\"row collapse\">\n            <div class=\"columns small-6\">\n                <label for=\"select-tool\">Start</label>\n                <input type=\"date\" name=\"start_date\" value=\"2022-11-23\" required>\n                <small class=\"error\">Select Date</small>\n            </div>\n            \n            <div class=\"columns small-6\">\n\t\t\t\t<label for=\"select-tool\" class=\"right\">End</label>\n                <input type=\"date\" name=\"end_date\" value=\"2022-11-30\" required>\n                <small class=\"error\">Select Date</small>\n            </div>\n        </div>\n\n       \n        <div class=\"row\">\n            <div class=\"columns\">\n            \t<button type=\"submit\" class=\"small right secondary radius\" id=\"btn-view-schedule\" >Check</button>\n            \t<span class=\"has-tooltip\" title=\"subscribe to this schedule\" ></span>\n                <input type=\"hidden\" name=\"nonce\" value=\"9xNGZda%lDKYFbV7zxFS\">\n                <input type=\"hidden\" name=\"nonce_key\" value=\"booking-view-tool-1669869926\">\n                \n            </div>\n        </div> \n\t</form>\n<div id=\"ajax-booking-view-tool-1669869926\" name=\"results\" data-alert></div>\n</div></div>\t</div>\n</div>";
    const RESULT1: &str = "Element { name: \"div\", attrs: {\"class\": \"row\"} }\n    Element { name: \"div\", attrs: {\"class\": \"columns\"} }\n        Element { name: \"div\", attrs: {\"class\": \"row\"} }\n            Element { name: \"div\", attrs: {\"class\": \"columns\"} }\n                Element { name: \"form\", attrs: {\"action\": \"ajax.get-bookings.php\", \"class\": \"custom\", \"data-abide\": \"\", \"id\": \"frm-update-date-range\", \"name\": \"frm-update-date-range\", \"target\": \"#ajax-booking-view-tool-1669869926\"} }\n                    Element { name: \"div\", attrs: {\"class\": \"row \"} }\n                        Element { name: \"div\", attrs: {\"class\": \"columns\"} }\n                            Element { name: \"label\", attrs: {} }\n                                Select Tool(s)\n                            Element { name: \"input\", attrs: {\"class\": \"select2-ajax\", \"data-placeholder\": \"Select Tools.. (leave blank for all tools)\", \"hide_inactive\": \"1\", \"multiple\": \"multiple\", \"name\": \"tool_id[]\", \"source\": \"ajax.get-tools.php\", \"type\": \"hidden\", \"value\": \"\"} }\n                            Element { name: \"small\", attrs: {\"class\": \"error\"} }\n                                Pick some tools\n                    Element { name: \"div\", attrs: {\"class\": \"row collapse\"} }\n                        Element { name: \"div\", attrs: {\"class\": \"columns small-6\"} }\n                            Element { name: \"label\", attrs: {\"for\": \"select-tool\"} }\n                                Start\n                            Element { name: \"input\", attrs: {\"name\": \"start_date\", \"required\": \"\", \"type\": \"date\", \"value\": \"2022-11-23\"} }\n                            Element { name: \"small\", attrs: {\"class\": \"error\"} }\n                                Select Date\n                        Element { name: \"div\", attrs: {\"class\": \"columns small-6\"} }\n                            Element { name: \"label\", attrs: {\"class\": \"right\", \"for\": \"select-tool\"} }\n                                End\n                            Element { name: \"input\", attrs: {\"name\": \"end_date\", \"required\": \"\", \"type\": \"date\", \"value\": \"2022-11-30\"} }\n                            Element { name: \"small\", attrs: {\"class\": \"error\"} }\n                                Select Date\n                    Element { name: \"div\", attrs: {\"class\": \"row\"} }\n                        Element { name: \"div\", attrs: {\"class\": \"columns\"} }\n                            Element { name: \"button\", attrs: {\"class\": \"small right secondary radius\", \"id\": \"btn-view-schedule\", \"type\": \"submit\"} }\n                                Check\n                            Element { name: \"span\", attrs: {\"class\": \"has-tooltip\", \"title\": \"subscribe to this schedule\"} }\n                            Element { name: \"input\", attrs: {\"name\": \"nonce\", \"type\": \"hidden\", \"value\": \"9xNGZda%lDKYFbV7zxFS\"} }\n                            Element { name: \"input\", attrs: {\"name\": \"nonce_key\", \"type\": \"hidden\", \"value\": \"booking-view-tool-1669869926\"} }\n                Element { name: \"div\", attrs: {\"data-alert\": \"\", \"id\": \"ajax-booking-view-tool-1669869926\", \"name\": \"results\"} }";
    const TEST2: &str = "<div class=\"section-container accordion\" data-section=\"accordion\">	<section class=\"active\">		<p class=\"title\" data-section-title><a href=\"\">Thu, Nov 24</a> </p> <div class=\"content\"					data-section-content>					<h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191730\" class=\"table-row group-8ae908785e3a1bb237ea2641a043a4b0\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:00am Thu Nov 24th\" data-tooltip> 6:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Thu Nov 24th\" data-tooltip> 9:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Wyatt James <br/> wyatt@norcada.com\" data-tooltip >wjames</span></div></div></div><div id=\"booking-190677\" class=\"table-row group-76f1e7763eaa4d882813a63cf5d51f8d\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Thu Nov 24th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:10am Thu Nov 24th\" data-tooltip> 11:10 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Min Wu <br/> wu2@ualberta.ca\" data-tooltip >min-wu</span></div></div></div><div id=\"booking-191536\" class=\"table-row group-293df35ed63cb7ccbe156a3d79908973\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:30am Thu Nov 24th\" data-tooltip> 11:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:00pm Thu Nov 24th\" data-tooltip> 13:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Daksh Malhotra <br/> dmalhot2@ualberta.ca\" data-tooltip >dmalhot2</span></div></div></div><div id=\"booking-191132\" class=\"table-row group-41cee0e4055e0ac8184ae497014c49be\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Thu Nov 24th\" data-tooltip> 14:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Thu Nov 24th\" data-tooltip> 16:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Kevin Setzer <br/> kevin@appliednt.com\" data-tooltip >ksetzer</span></div></div></div><div id=\"booking-191677\" class=\"table-row group-5b68a4ec1e52b15f75b7bcd6f4224872\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Thu Nov 24th\" data-tooltip> 16:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"5:00pm Thu Nov 24th\" data-tooltip> 17:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Daniel Mildenberger <br/> dmildenb@ualberta.ca\" data-tooltip >dmildenb</span></div></div></div><div id=\"booking-191678\" class=\"table-row group-4b8a09b6d673022ecf5de68c575fe717\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"5:00pm Thu Nov 24th\" data-tooltip> 17:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:00pm Thu Nov 24th\" data-tooltip> 18:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Daniel Mildenberger <br/> dmildenb@ualberta.ca\" data-tooltip >dmildenb</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Fri, Nov 25</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191701\" class=\"table-row group-761fa754ca075f0663e471d6b98141f4\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:00am Fri Nov 25th\" data-tooltip> 6:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"7:00am Fri Nov 25th\" data-tooltip> 7:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Wyatt James <br/> wyatt@norcada.com\" data-tooltip >wjames</span></div></div></div><div id=\"booking-191542\" class=\"table-row group-3da11a1ebb5bccceb4f9a6fa3ee5a9f2\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"7:00am Fri Nov 25th\" data-tooltip> 7:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Fri Nov 25th\" data-tooltip> 9:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Pedro Duarte Riveros <br/> duarteri@ualberta.ca\" data-tooltip >Duarteri</span></div></div></div><div id=\"booking-190689\" class=\"table-row group-ac7627ab3efd68204626b1133a7b9f06\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Fri Nov 25th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Fri Nov 25th\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div><div id=\"booking-191571\" class=\"table-row group-1bd94fc24bc77a9c28737553cbbdde5c\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Fri Nov 25th\" data-tooltip> 11:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:30pm Fri Nov 25th\" data-tooltip> 13:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Abbie Gottert <br/> abbie@norcada.com\" data-tooltip >agottert</span></div></div></div><div id=\"booking-191134\" class=\"table-row group-378f68e0c942509b067cf55bad7a404e\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:30pm Fri Nov 25th\" data-tooltip> 13:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"3:30pm Fri Nov 25th\" data-tooltip> 15:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Kevin Setzer <br/> kevin@appliednt.com\" data-tooltip >ksetzer</span></div></div></div><div id=\"booking-191602\" class=\"table-row group-5ed742c71f7260f261bd931d56e18f51\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Fri Nov 25th\" data-tooltip> 16:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"10:00pm Fri Nov 25th\" data-tooltip> 22:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Ahmed Elsherbiny <br/> ahmed.elsherbiny@shebamicrosystems.com\" data-tooltip >elsherbi</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Sat, Nov 26</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191702\" class=\"table-row group-402d78fd8e4fcee5e4d53a5ba6a39a92\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:00am Sat Nov 26th\" data-tooltip> 6:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"8:30am Sat Nov 26th\" data-tooltip> 8:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Wyatt James <br/> wyatt@norcada.com\" data-tooltip >wjames</span></div></div></div><div id=\"booking-191653\" class=\"table-row group-b357212ef36db734ec877d14d45575f7\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"8:30am Sat Nov 26th\" data-tooltip> 8:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"10:00am Sat Nov 26th\" data-tooltip> 10:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Alexandria McKinlay <br/> amckinlay@appliednt.com\" data-tooltip >amckinlay</span></div></div></div><div id=\"booking-191703\" class=\"table-row group-89de0efb3491a3f70030d030e05442d4\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"10:00am Sat Nov 26th\" data-tooltip> 10:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"12:00pm Sat Nov 26th\" data-tooltip> 12:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Wyatt James <br/> wyatt@norcada.com\" data-tooltip >wjames</span></div></div></div><div id=\"booking-191426\" class=\"table-row group-0d62f43375be21685f2c82f24972cd6c\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Sat Nov 26th\" data-tooltip> 14:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Sat Nov 26th\" data-tooltip> 16:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Sun, Nov 27</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191603\" class=\"table-row group-b125d24de84d1f18fe5abbf674121684\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Sun Nov 27th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:00pm Sun Nov 27th\" data-tooltip> 13:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Ahmed Elsherbiny <br/> ahmed.elsherbiny@shebamicrosystems.com\" data-tooltip >elsherbi</span></div></div></div><div id=\"booking-191938\" class=\"table-row group-97e22a4d44845d631319369782426a13\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:30pm Sun Nov 27th\" data-tooltip> 16:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:30pm Sun Nov 27th\" data-tooltip> 18:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div><div id=\"booking-191939\" class=\"table-row group-77f99f37c34a6997a85ebae7f31d46c8\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:30pm Sun Nov 27th\" data-tooltip> 18:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"8:30pm Sun Nov 27th\" data-tooltip> 20:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Mon, Nov 28</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191576\" class=\"table-row group-fe2b75dcb307e5f687ba5fc11c71d8e8\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"7:00am Mon Nov 28th\" data-tooltip> 7:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Mon Nov 28th\" data-tooltip> 9:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Pedro Duarte Riveros <br/> duarteri@ualberta.ca\" data-tooltip >Duarteri</span></div></div></div><div id=\"booking-190707\" class=\"table-row group-e0b463a324cea71f7278407e04ab166d\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"12:00pm Mon Nov 28th\" data-tooltip> 12:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Mon Nov 28th\" data-tooltip> 14:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Min Wu <br/> wu2@ualberta.ca\" data-tooltip >min-wu</span></div></div></div><div id=\"booking-191809\" class=\"table-row group-5974cffcc961333e9d68e257282b0d98\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Mon Nov 28th\" data-tooltip> 14:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Mon Nov 28th\" data-tooltip> 16:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Daniel Mildenberger <br/> dmildenb@ualberta.ca\" data-tooltip >dmildenb</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Tue, Nov 29</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191577\" class=\"table-row group-979074d9d3f8c82821da62b5eab85c5d\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"7:00am Tue Nov 29th\" data-tooltip> 7:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Tue Nov 29th\" data-tooltip> 9:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Pedro Duarte Riveros <br/> duarteri@ualberta.ca\" data-tooltip >Duarteri</span></div></div></div><div id=\"booking-191257\" class=\"table-row group-f04015d3be04dba84953a3cafad6d5c6\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Tue Nov 29th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Tue Nov 29th\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div><div id=\"booking-191336\" class=\"table-row group-8a1c073486cb52d05cf9c73dcaccf090\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Tue Nov 29th\" data-tooltip> 11:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:00pm Tue Nov 29th\" data-tooltip> 13:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Cory Rewcastle <br/> cory@transeon.ca\" data-tooltip >crewcastle</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Wed, Nov 30</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191740\" class=\"table-row group-eae5dd63e9c0f87a89b3619277c86cf8\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"10:00am Wed Nov 30th\" data-tooltip> 10:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"12:00pm Wed Nov 30th\" data-tooltip> 12:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Cory Rewcastle <br/> cory@transeon.ca\" data-tooltip >crewcastle</span></div></div></div><div id=\"booking-190681\" class=\"table-row group-7f2cc60a903d4dbcd83bdc66cd4ce082\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:30pm Wed Nov 30th\" data-tooltip> 13:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"3:30pm Wed Nov 30th\" data-tooltip> 15:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div><div id=\"booking-191270\" class=\"table-row group-5542d9b02f97f3bd12e9e1762f90e86b\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Wed Nov 30th\" data-tooltip> 16:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"6:00pm Wed Nov 30th\" data-tooltip> 18:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Min Wu <br/> wu2@ualberta.ca\" data-tooltip >min-wu</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Thu, Dec 1</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-190698\" class=\"table-row group-b51f070ebe4f5b78c63ceb66de19324f\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Thu Dec 1st\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Thu Dec 1st\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div><div id=\"booking-191346\" class=\"table-row group-213fa1f6f5c4d9309e1700676dcbe4cf\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Thu Dec 1st\" data-tooltip> 11:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:00pm Thu Dec 1st\" data-tooltip> 13:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Cory Rewcastle <br/> cory@transeon.ca\" data-tooltip >crewcastle</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Mon, Dec 5</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191275\" class=\"table-row group-f52c97502eb34eebc15bc231ba4f2e33\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Mon Dec 5th\" data-tooltip> 14:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"4:00pm Mon Dec 5th\" data-tooltip> 16:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Wed, Dec 7</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-191281\" class=\"table-row group-ecefe6746df8464aaf0af04bb919b72a\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Wed Dec 7th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Wed Dec 7th\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div><div id=\"booking-190685\" class=\"table-row group-85c33a95a664b0c7478022fd2ad4d837\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:30pm Wed Dec 7th\" data-tooltip> 13:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"3:30pm Wed Dec 7th\" data-tooltip> 15:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Thu, Dec 8</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-190699\" class=\"table-row group-e0b5728e888dc7948946dcd3fcca1caf\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Thu Dec 8th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Thu Dec 8th\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div><div id=\"booking-191286\" class=\"table-row group-89fbf79710e8645968f969278e718db2\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"12:00pm Thu Dec 8th\" data-tooltip> 12:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"2:00pm Thu Dec 8th\" data-tooltip> 14:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Eric Milburn <br/> eric@zinite.com\" data-tooltip >emilburn</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Wed, Dec 14</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-190686\" class=\"table-row group-c0fa0d01811c2d074a34a1cb7ebfc137\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"1:30pm Wed Dec 14th\" data-tooltip> 13:30</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"3:30pm Wed Dec 14th\" data-tooltip> 15:30 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section><section ><p class=\"title\" data-section-title><a href=\"\">Thu, Dec 15</a></p><div class=\"content\" data-section-content><h4><small><a href=\"equipment-detail.php?tool_id=427\">Heidelberg MLA150</a></small></h4><div class=\"row\"><div class=\"columns\"><div class=\"table\">\r\n\t\t\t\t\t\t\t\t<div class=\"table-head\">\r\n\t\t\t\t\t\t\t\t\t<div class=\"row table-row\">\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">start</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 \">stop</div>\r\n\t\t\t\t\t\t\t\t\t\t<div class=\"columns small-4 left\">user</div>\r\n\t\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t</div>\r\n\t\t\t\t\t\t\t\t<div class=\"table-body\" ><div id=\"booking-190700\" class=\"table-row group-032074d744988726d6e4e90acf98f994\"><div class=\"row\"><div class=\"columns small-4\"><span class=\"has-tip\" title=\"9:00am Thu Dec 15th\" data-tooltip> 9:00</span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"11:00am Thu Dec 15th\" data-tooltip> 11:00 </span></div><div class=\"columns small-4\"><span class=\"has-tip\" title=\"Gustavo de Oliveira Luiz <br/> deolivei@ualberta.ca\" data-tooltip >gde-oliveira-luiz</span></div></div></div></div></div><div id=\"ajax-rb-427\" name=\"results\" data-fade-out ></div></section></div>";
}
