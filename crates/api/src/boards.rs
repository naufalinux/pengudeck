use hermes_dash_core::source::SourceError;

pub trait BoardSource {
    fn boards(&self) -> Result<Vec<String>, SourceError>;
}

pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub fn parse_boards(input: &str) -> Result<Vec<String>, SourceError> {
    let mut lines = input.lines().map(str::trim).filter(|line| !line.is_empty());
    if lines
        .next()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        != Some(vec!["SLUG", "NAME", "COUNTS"])
    {
        return Err(SourceError::InvalidData);
    }
    let mut boards = Vec::new();
    for line in lines {
        if line.starts_with("Current board: ")
            || line.starts_with("Switch boards with `hermes kanban boards switch ")
        {
            continue;
        }
        let fields: Vec<_> = line
            .strip_prefix('●')
            .unwrap_or(line)
            .split_whitespace()
            .collect();
        if fields.len() < 3
            || !valid_slug(fields[0])
            || !fields.last().is_some_and(|value| {
                value.split_once('=').is_some_and(|(key, count)| {
                    valid_slug(key) && count.trim_end_matches(',').parse::<u64>().is_ok()
                })
            })
        {
            return Err(SourceError::InvalidData);
        }
        if !boards.iter().any(|slug| slug == fields[0]) {
            boards.push(fields[0].to_owned());
        }
    }
    Ok(boards)
}
