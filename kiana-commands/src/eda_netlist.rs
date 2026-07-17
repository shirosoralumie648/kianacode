use anyhow::{anyhow, Context, Result};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NetlistIssue {
    pub code: &'static str,
    pub severity: &'static str,
    pub message: String,
    pub evidence: Vec<String>,
    pub designator: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NetlistAnalysis {
    pub component_refs: BTreeSet<String>,
    pub component_count: usize,
    pub net_count: usize,
    pub power_net_count: usize,
    pub interface_net_count: usize,
    pub dangling_net_count: usize,
    pub issues: Vec<NetlistIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetNode {
    reference: String,
    pin: String,
    pin_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetRecord {
    code: String,
    name: String,
    nodes: Vec<NetNode>,
}

pub(crate) fn analyze_kicad_netlist(bytes: &[u8]) -> Result<NetlistAnalysis> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = true;

    let mut buffer = Vec::new();
    let mut depth = 0_usize;
    let mut saw_export = false;
    let mut closed_export = false;
    let mut saw_components = false;
    let mut saw_nets = false;
    let mut in_components = false;
    let mut in_nets = false;
    let mut component_refs = BTreeSet::new();
    let mut net_codes = BTreeSet::new();
    let mut nets = Vec::new();
    let mut current_net: Option<NetRecord> = None;

    loop {
        match reader
            .read_event_into(&mut buffer)
            .context("failed to parse KiCad netlist XML")?
        {
            Event::Decl(_) | Event::Comment(_) => {}
            Event::Text(text) => {
                let raw: &[u8] = text.as_ref();
                if depth == 0 && raw.iter().any(|byte| !byte.is_ascii_whitespace()) {
                    return Err(anyhow!(
                        "KiCad netlist XML contains non-whitespace text outside <export>"
                    ));
                }
            }
            Event::CData(_) if depth == 0 => {
                return Err(anyhow!("KiCad netlist XML contains CDATA outside <export>"));
            }
            Event::CData(_) => {}
            Event::Start(start) => {
                let name = start.name();
                if depth == 0 {
                    if name.as_ref() != b"export" || saw_export || closed_export {
                        return Err(anyhow!("KiCad netlist XML root must be <export>"));
                    }
                    saw_export = true;
                } else if !saw_export || closed_export {
                    return Err(anyhow!("KiCad netlist XML contains data outside <export>"));
                }
                process_start(
                    &reader,
                    &start,
                    false,
                    depth,
                    &mut in_components,
                    &mut in_nets,
                    &mut saw_components,
                    &mut saw_nets,
                    &mut component_refs,
                    &mut net_codes,
                    &mut nets,
                    &mut current_net,
                )?;
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("KiCad netlist XML nesting overflow"))?;
            }
            Event::Empty(start) => {
                if depth == 0 || !saw_export || closed_export {
                    return Err(anyhow!("KiCad netlist XML contains data outside <export>"));
                }
                process_start(
                    &reader,
                    &start,
                    true,
                    depth,
                    &mut in_components,
                    &mut in_nets,
                    &mut saw_components,
                    &mut saw_nets,
                    &mut component_refs,
                    &mut net_codes,
                    &mut nets,
                    &mut current_net,
                )?;
            }
            Event::End(end) => {
                if depth == 0 {
                    return Err(anyhow!(
                        "KiCad netlist XML has an unexpected closing element"
                    ));
                }
                let name = end.name();
                match name.as_ref() {
                    b"net" => {
                        let net = current_net.take().ok_or_else(|| {
                            anyhow!("KiCad netlist XML closed a net that was not open")
                        })?;
                        nets.push(net);
                    }
                    b"components" => in_components = false,
                    b"nets" => in_nets = false,
                    _ => {}
                }
                depth -= 1;
                if name.as_ref() == b"export" {
                    if depth != 0 {
                        return Err(anyhow!(
                            "KiCad netlist XML closed <export> at the wrong depth"
                        ));
                    }
                    closed_export = true;
                }
            }
            Event::Eof => break,
            Event::PI(_) => {
                return Err(anyhow!(
                    "KiCad netlist XML contains a processing instruction"
                ));
            }
            Event::DocType(_) => {
                return Err(anyhow!("KiCad netlist XML contains a DOCTYPE"));
            }
            Event::GeneralRef(_) => {
                return Err(anyhow!(
                    "KiCad netlist XML contains a general entity reference"
                ));
            }
        }
        buffer.clear();
    }

    if !saw_export || !closed_export || depth != 0 {
        return Err(anyhow!("KiCad netlist XML ended before </export>"));
    }
    if !saw_components {
        return Err(anyhow!("KiCad netlist XML is missing <components>"));
    }
    if !saw_nets {
        return Err(anyhow!("KiCad netlist XML is missing <nets>"));
    }
    if current_net.is_some() {
        return Err(anyhow!("KiCad netlist XML ended with an open net"));
    }

    nets.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.name.cmp(&right.name))
    });
    let mut analysis = NetlistAnalysis {
        component_count: component_refs.len(),
        net_count: nets.len(),
        component_refs,
        ..NetlistAnalysis::default()
    };

    for net in nets {
        for node in &net.nodes {
            if !analysis.component_refs.contains(&node.reference) {
                return Err(anyhow!(
                    "KiCad netlist XML node references unknown component {} on net {}",
                    node.reference,
                    net.name
                ));
            }
        }
        inspect_net(&net, &mut analysis);
    }
    analysis.issues.sort_by(|left, right| {
        left.code
            .cmp(right.code)
            .then_with(|| left.evidence.cmp(&right.evidence))
            .then_with(|| left.designator.cmp(&right.designator))
    });
    Ok(analysis)
}

#[allow(clippy::too_many_arguments)]
fn process_start(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    empty: bool,
    depth: usize,
    in_components: &mut bool,
    in_nets: &mut bool,
    saw_components: &mut bool,
    saw_nets: &mut bool,
    component_refs: &mut BTreeSet<String>,
    net_codes: &mut BTreeSet<String>,
    nets: &mut Vec<NetRecord>,
    current_net: &mut Option<NetRecord>,
) -> Result<()> {
    let name = start.name();
    match name.as_ref() {
        b"components" => {
            if depth != 1 {
                return Err(anyhow!(
                    "KiCad netlist XML <components> must be a direct child of <export>"
                ));
            }
            if *saw_components {
                return Err(anyhow!("KiCad netlist XML contains duplicate <components>"));
            }
            *saw_components = true;
            if !empty {
                *in_components = true;
            }
        }
        b"nets" => {
            if depth != 1 {
                return Err(anyhow!(
                    "KiCad netlist XML <nets> must be a direct child of <export>"
                ));
            }
            if *saw_nets {
                return Err(anyhow!("KiCad netlist XML contains duplicate <nets>"));
            }
            *saw_nets = true;
            if !empty {
                *in_nets = true;
            }
        }
        b"comp" => {
            if !*in_components || depth != 2 {
                return Err(anyhow!(
                    "KiCad netlist XML <comp> must be a direct child of <components>"
                ));
            }
            let reference =
                normalize_designator(&required_attribute(reader, start, b"ref", "component ref")?);
            if !component_refs.insert(reference.clone()) {
                return Err(anyhow!(
                    "KiCad netlist XML contains duplicate component {reference}"
                ));
            }
        }
        b"net" => {
            if !*in_nets || depth != 2 {
                return Err(anyhow!(
                    "KiCad netlist XML <net> must be a direct child of <nets>"
                ));
            }
            if current_net.is_some() {
                return Err(anyhow!("KiCad netlist XML contains nested nets"));
            }
            let code = required_attribute(reader, start, b"code", "net code")?;
            let name = required_attribute(reader, start, b"name", "net name")?;
            if !net_codes.insert(code.clone()) {
                return Err(anyhow!(
                    "KiCad netlist XML contains duplicate net code {code}"
                ));
            }
            let net = NetRecord {
                code,
                name,
                nodes: Vec::new(),
            };
            if empty {
                nets.push(net);
            } else {
                *current_net = Some(net);
            }
        }
        b"node" => {
            if depth != 3 {
                return Err(anyhow!(
                    "KiCad netlist XML <node> must be a direct child of <net>"
                ));
            }
            let net = current_net
                .as_mut()
                .ok_or_else(|| anyhow!("KiCad netlist XML contains a node outside a net"))?;
            let reference =
                normalize_designator(&required_attribute(reader, start, b"ref", "node ref")?);
            let pin = required_attribute(reader, start, b"pin", "node pin")?;
            if net
                .nodes
                .iter()
                .any(|node| node.reference == reference && node.pin == pin)
            {
                return Err(anyhow!(
                    "KiCad netlist XML contains duplicate node {reference}:{pin} on net {}",
                    net.name
                ));
            }
            net.nodes.push(NetNode {
                reference,
                pin,
                pin_type: optional_attribute(reader, start, b"pintype")?
                    .unwrap_or_default()
                    .trim()
                    .to_ascii_lowercase(),
            });
        }
        _ => {}
    }
    Ok(())
}

fn required_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    key: &[u8],
    label: &str,
) -> Result<String> {
    let value = optional_attribute(reader, start, key)?
        .ok_or_else(|| anyhow!("KiCad netlist XML is missing {label}"))?;
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("KiCad netlist XML is missing {label}"));
    }
    Ok(value.to_string())
}

fn normalize_designator(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn optional_attribute(
    reader: &Reader<&[u8]>,
    start: &BytesStart<'_>,
    key: &[u8],
) -> Result<Option<String>> {
    for attribute in start.attributes() {
        let attribute = attribute.context("invalid KiCad netlist XML attribute")?;
        if attribute.key.as_ref() == key {
            return Ok(Some(
                attribute
                    .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                    .context("invalid KiCad netlist XML attribute value")?
                    .into_owned(),
            ));
        }
    }
    Ok(None)
}

fn inspect_net(net: &NetRecord, analysis: &mut NetlistAnalysis) {
    let power = is_power_net(net);
    let interface = is_interface_net(&net.name);
    let no_connect = is_no_connect_net(&net.name);
    if power {
        analysis.power_net_count += 1;
    }
    if interface {
        analysis.interface_net_count += 1;
    }
    if net.nodes.is_empty() {
        analysis.issues.push(NetlistIssue {
            code: "netlist_empty_net",
            severity: "error",
            message: format!("Net {} has no connected nodes.", net.name),
            evidence: vec![net.name.clone()],
            designator: None,
        });
        return;
    }
    if net.nodes.len() == 1 && !no_connect {
        analysis.dangling_net_count += 1;
        analysis.issues.push(NetlistIssue {
            code: if interface {
                "netlist_interface_dangling"
            } else {
                "netlist_dangling_net"
            },
            severity: "warning",
            message: format!("Net {} has only one connected node.", net.name),
            evidence: vec![format!("{}:{}", net.nodes[0].reference, net.nodes[0].pin)],
            designator: Some(net.nodes[0].reference.clone()),
        });
    }

    if power {
        let power_in = net
            .nodes
            .iter()
            .filter(|node| node.pin_type == "power_in")
            .count();
        let power_out = net
            .nodes
            .iter()
            .filter(|node| node.pin_type == "power_out")
            .count();
        if power_out > 1 {
            analysis.issues.push(NetlistIssue {
                code: "netlist_power_driver_conflict",
                severity: "warning",
                message: format!("Power net {} has {power_out} power_out nodes.", net.name),
                evidence: net
                    .nodes
                    .iter()
                    .filter(|node| node.pin_type == "power_out")
                    .map(|node| format!("{}:{}", node.reference, node.pin))
                    .collect(),
                designator: None,
            });
        }
        if power_in > 0 && power_out == 0 && !is_ground_net(&net.name) {
            analysis.issues.push(NetlistIssue {
                code: "netlist_power_source_unresolved",
                severity: "warning",
                message: format!(
                    "Power net {} has power_in nodes but no explicit power_out node.",
                    net.name
                ),
                evidence: net
                    .nodes
                    .iter()
                    .filter(|node| node.pin_type == "power_in")
                    .map(|node| format!("{}:{}", node.reference, node.pin))
                    .collect(),
                designator: None,
            });
        }
    }
}

fn is_power_net(net: &NetRecord) -> bool {
    if net
        .nodes
        .iter()
        .any(|node| matches!(node.pin_type.as_str(), "power_in" | "power_out"))
    {
        return true;
    }
    let name = net.name.trim().to_ascii_uppercase();
    is_ground_net(&name)
        || name.starts_with('+')
        || name.starts_with('-')
        || ["VCC", "VDD", "VSS", "VBAT", "VIN", "VOUT"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || looks_like_voltage_rail(&name)
}

fn looks_like_voltage_rail(name: &str) -> bool {
    let compact = name
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>();
    let Some(v_index) = compact.find('V') else {
        return false;
    };
    v_index > 0
        && compact[..v_index].chars().all(|ch| ch.is_ascii_digit())
        && compact[v_index + 1..].chars().all(|ch| ch.is_ascii_digit())
}

fn is_ground_net(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_uppercase().as_str(),
        "GND" | "AGND" | "DGND" | "PGND" | "GROUND"
    )
}

fn is_interface_net(name: &str) -> bool {
    let upper = name.trim().to_ascii_uppercase();
    let normalized = upper
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let tokens = normalized
        .split('_')
        .filter(|token| !token.is_empty())
        .collect::<BTreeSet<_>>();
    [
        "USB", "I2C", "SDA", "SCL", "SPI", "MOSI", "MISO", "SCK", "UART", "CAN", "SWD", "SWDIO",
        "SWCLK", "JTAG",
    ]
    .iter()
    .any(|keyword| upper.contains(keyword))
        || tokens.contains("TX")
        || tokens.contains("RX")
}

fn is_no_connect_net(name: &str) -> bool {
    let normalized = name.trim().to_ascii_uppercase();
    normalized.contains("UNCONNECTED")
        || normalized.contains("NO_CONNECT")
        || normalized == "N/C"
        || normalized == "NC"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_power_and_interface_networks() {
        let analysis = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/><comp ref="R1"/></components><nets><net code="1" name="+3V3"><node ref="U1" pin="1" pintype="power_in"/><node ref="U1" pin="2" pintype="power_out"/></net><net code="2" name="SWDIO"><node ref="U1" pin="3"/><node ref="R1" pin="1"/></net></nets></export>"#,
        )
        .unwrap();

        assert_eq!(analysis.component_count, 2);
        assert_eq!(analysis.net_count, 2);
        assert_eq!(analysis.power_net_count, 1);
        assert_eq!(analysis.interface_net_count, 1);
        assert_eq!(analysis.dangling_net_count, 0);
        assert!(analysis.issues.is_empty());
    }

    #[test]
    fn rejects_unknown_component_references() {
        let error = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/></components><nets><net code="1" name="SWDIO"><node ref="J1" pin="1"/></net></nets></export>"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("unknown component J1"));
    }

    #[test]
    fn reports_dangling_interface_and_unresolved_power() {
        let analysis = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/></components><nets><net code="1" name="USB_D+"><node ref="U1" pin="1"/></net><net code="2" name="+3V3"><node ref="U1" pin="2" pintype="power_in"/></net></nets></export>"#,
        )
        .unwrap();

        assert_eq!(analysis.dangling_net_count, 2);
        assert!(analysis
            .issues
            .iter()
            .any(|issue| issue.code == "netlist_interface_dangling"));
        assert!(analysis
            .issues
            .iter()
            .any(|issue| issue.code == "netlist_power_source_unresolved"));
    }

    #[test]
    fn rejects_duplicate_component_and_net_identities() {
        let component_error = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/><comp ref="U1"/></components><nets></nets></export>"#,
        )
        .unwrap_err();
        assert!(component_error
            .to_string()
            .contains("duplicate component U1"));

        let net_error = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/></components><nets><net code="1" name="A"/><net code="1" name="B"/></nets></export>"#,
        )
        .unwrap_err();
        assert!(net_error.to_string().contains("duplicate net code 1"));
    }

    #[test]
    fn rejects_required_sections_outside_export_root() {
        let error = analyze_kicad_netlist(
            br#"<export><wrapper><components><comp ref="U1"/></components></wrapper><nets></nets></export>"#,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("<components> must be a direct child of <export>"));
    }

    #[test]
    fn rejects_duplicate_required_sections() {
        let components_error = analyze_kicad_netlist(
            br#"<export><components></components><components></components><nets></nets></export>"#,
        )
        .unwrap_err();
        assert!(components_error
            .to_string()
            .contains("duplicate <components>"));

        let nets_error = analyze_kicad_netlist(
            br#"<export><components></components><nets></nets><nets></nets></export>"#,
        )
        .unwrap_err();
        assert!(nets_error.to_string().contains("duplicate <nets>"));
    }

    #[test]
    fn rejects_doctype_processing_instruction_and_general_reference() {
        for (xml, expected) in [
            (
                br#"<!DOCTYPE export><export><components></components><nets></nets></export>"#.as_slice(),
                "DOCTYPE",
            ),
            (
                br#"<?unsafe data?><export><components></components><nets></nets></export>"#.as_slice(),
                "processing instruction",
            ),
            (
                br#"<export><components><comp ref="U1"><value>&custom;</value></comp></components><nets></nets></export>"#.as_slice(),
                "general entity reference",
            ),
        ] {
            let error = analyze_kicad_netlist(xml).unwrap_err();
            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }

    #[test]
    fn normalizes_component_references_and_rejects_duplicate_nodes() {
        let analysis = analyze_kicad_netlist(
            br#"<export><components><comp ref=" u1 "/></components><nets><net code="1" name="SWDIO"><node ref="U1" pin="1"/></net></nets></export>"#,
        )
        .unwrap();
        assert!(analysis.component_refs.contains("U1"));

        let error = analyze_kicad_netlist(
            br#"<export><components><comp ref="U1"/></components><nets><net code="1" name="SWDIO"><node ref="U1" pin="1"/><node ref="u1" pin="1"/></net></nets></export>"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("duplicate node U1:1"));
    }
}
