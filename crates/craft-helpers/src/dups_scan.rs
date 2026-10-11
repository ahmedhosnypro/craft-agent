//! Window hashing: grouping, overlap merge, deterministic ranking.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::dups::{
    Candidate, DupsQuery, DupsReport, MAX_CANDIDATES, MAX_MIN_LINES, MAX_WINDOWS, Occurrence,
    ScanFile, collect_files,
};
use crate::error::HelperError;

struct FileRef {
    root: String,
    file: ScanFile,
}

#[derive(Clone, Copy)]
struct Hit {
    file: usize,
    index: usize,
}

/// A merged run of window hits inside one file, in kept-line indices.
struct Span {
    file: usize,
    first: usize,
    last: usize,
}

struct Group {
    total: usize,
    first_key: (String, String, usize),
    candidate: Candidate,
}

/// Hash every window of `min_lines` kept lines; report groups that repeat
/// across files or disjointly inside one file, merged and ranked.
pub fn scan_dups(query: &DupsQuery) -> Result<DupsReport, HelperError> {
    if !(2..=MAX_MIN_LINES).contains(&query.min_lines) {
        return Err(HelperError::InvalidMinLines(query.min_lines));
    }
    let mut skipped: Vec<String> = Vec::new();
    let mut files: Vec<FileRef> = Vec::new();
    for root in &query.roots {
        let label = root.to_string_lossy().to_string();
        for file in collect_files(root, &mut skipped) {
            files.push(FileRef {
                root: label.clone(),
                file,
            });
        }
    }
    let mut windows: HashMap<u64, Vec<Hit>> = HashMap::new();
    hash_windows(&files, query.min_lines, &mut windows, &mut skipped);
    let candidates = rank_groups(&files, &windows, query.min_lines, query.max_candidates);
    Ok(DupsReport {
        candidates,
        skipped,
    })
}

fn hash_windows(
    files: &[FileRef],
    min_lines: usize,
    windows: &mut HashMap<u64, Vec<Hit>>,
    skipped: &mut Vec<String>,
) {
    let mut total: usize = 0;
    'scan: for (file_index, record) in files.iter().enumerate() {
        let kept = &record.file.kept;
        if kept.len() < min_lines {
            continue;
        }
        for start in 0..=kept.len() - min_lines {
            if total >= MAX_WINDOWS {
                skipped.push("window cap reached; scan truncated".to_string());
                break 'scan;
            }
            let mut hasher = DefaultHasher::new();
            for line in &kept[start..start + min_lines] {
                line.text.hash(&mut hasher);
            }
            let hash = hasher.finish();
            match windows.get_mut(&hash) {
                Some(hits) => hits.push(Hit {
                    file: file_index,
                    index: start,
                }),
                None => {
                    windows.insert(
                        hash,
                        vec![Hit {
                            file: file_index,
                            index: start,
                        }],
                    );
                }
            }
            total += 1;
        }
    }
}

fn find(parent: &mut [usize], id: usize) -> usize {
    let mut current = id;
    loop {
        let Some(&up) = parent.get(current) else {
            return current;
        };
        if up == current {
            return current;
        }
        let Some(&up_up) = parent.get(up) else {
            return current;
        };
        parent[current] = up_up;
        current = up_up;
    }
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let a = find(parent, left);
    let b = find(parent, right);
    if a != b {
        parent[a.max(b)] = a.min(b);
    }
}

/// One span per merged run of a qualifying group; runs of one group are
/// unioned so overlapping windows of the same block collapse later.
fn collect_spans(windows: &HashMap<u64, Vec<Hit>>, min_lines: usize) -> (Vec<Span>, Vec<usize>) {
    let mut spans: Vec<Span> = Vec::new();
    let mut parent: Vec<usize> = Vec::new();
    for hits in windows.values() {
        if hits.len() < 2 {
            continue;
        }
        let mut sorted: Vec<Hit> = hits.to_vec();
        sorted.sort_by_key(|hit| (hit.file, hit.index));
        let mut runs: Vec<Hit> = Vec::new();
        for hit in sorted {
            let extends = matches!(runs.last(), Some(run)
                if run.file == hit.file && hit.index <= run.index.saturating_add(1));
            if extends {
                if let Some(run) = runs.last_mut() {
                    run.index = run.index.max(hit.index);
                }
            } else {
                runs.push(hit);
            }
        }
        let mut distinct = 0;
        let mut previous: Option<usize> = None;
        for run in &runs {
            if previous != Some(run.file) {
                distinct += 1;
                previous = Some(run.file);
            }
        }
        if distinct < 2 && runs.len() < 2 {
            continue;
        }
        let mut group: Vec<usize> = Vec::with_capacity(runs.len());
        for run in &runs {
            let id = spans.len();
            spans.push(Span {
                file: run.file,
                first: run.index,
                last: run.index + min_lines - 1,
            });
            parent.push(id);
            group.push(id);
        }
        let mut linked = group.iter().copied();
        if let Some(first) = linked.next() {
            for id in linked {
                union(&mut parent, first, id);
            }
        }
    }
    (spans, parent)
}

/// Union spans that overlap or touch inside one file; the region extent
/// grows as the sorted walk advances so chains and nests both merge.
fn link_overlaps(spans: &[Span], parent: &mut [usize]) {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|id| (spans[*id].file, spans[*id].first, spans[*id].last));
    let mut region: Option<(usize, usize, usize)> = None;
    for id in order {
        let Some(span) = spans.get(id) else {
            continue;
        };
        let overlap = matches!(region, Some((file, extent, _))
            if file == span.file && span.first <= extent.saturating_add(1));
        match region {
            Some((file, extent, rep)) if overlap => {
                union(parent, rep, id);
                region = Some((file, extent.max(span.last), rep));
            }
            _ => region = Some((span.file, span.last, id)),
        }
    }
}

/// Every component becomes one candidate; per file, its spans chain into
/// regions and the widest region fixes the candidate's line count.
fn rank_groups(
    files: &[FileRef],
    windows: &HashMap<u64, Vec<Hit>>,
    min_lines: usize,
    max_candidates: usize,
) -> Vec<Candidate> {
    let (spans, mut parent) = collect_spans(windows, min_lines);
    link_overlaps(&spans, &mut parent);
    let cap = max_candidates.min(MAX_CANDIDATES);
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for id in 0..spans.len() {
        let root = find(&mut parent, id);
        components.entry(root).or_default().push(id);
    }
    let mut groups: Vec<Group> = Vec::new();
    for ids in components.values() {
        if let Some(group) = build_group(files, &spans, ids) {
            groups.push(group);
        }
    }
    groups.sort_by(|a, b| {
        b.total
            .cmp(&a.total)
            .then_with(|| a.first_key.cmp(&b.first_key))
    });
    groups
        .into_iter()
        .take(cap)
        .map(|group| group.candidate)
        .collect()
}

fn build_group(files: &[FileRef], spans: &[Span], ids: &[usize]) -> Option<Group> {
    let mut order: Vec<usize> = ids.to_vec();
    order.sort_by_key(|id| (spans[*id].file, spans[*id].first, spans[*id].last));
    let mut regions: Vec<(usize, usize, usize)> = Vec::new();
    for id in order {
        let Some(span) = spans.get(id) else {
            continue;
        };
        let overlap = matches!(regions.last(), Some(region)
            if region.0 == span.file && span.first <= region.2.saturating_add(1));
        match regions.last_mut() {
            Some(region) if overlap => {
                region.1 = region.1.min(span.first);
                region.2 = region.2.max(span.last);
            }
            _ => regions.push((span.file, span.first, span.last)),
        }
    }
    let lines = regions.iter().map(|region| region.2 - region.1 + 1).max()?;
    let mut occurrences: Vec<Occurrence> = Vec::with_capacity(regions.len());
    for (file, first, _) in &regions {
        let Some(record) = files.get(*file) else {
            continue;
        };
        let Some(kept) = record.file.kept.get(*first) else {
            continue;
        };
        occurrences.push(Occurrence {
            root: record.root.clone(),
            file: record.file.rel.clone(),
            line: kept.line,
        });
    }
    if occurrences.is_empty() {
        return None;
    }
    occurrences.sort_by(|a, b| (&a.root, &a.file, a.line).cmp(&(&b.root, &b.file, b.line)));
    let total = lines.saturating_mul(occurrences.len());
    let first = &occurrences[0];
    Some(Group {
        total,
        first_key: (first.root.clone(), first.file.clone(), first.line),
        candidate: Candidate { lines, occurrences },
    })
}
