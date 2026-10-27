//! The MR projection: sessions grouped by the bucket their pull requests add
//! up to, in the fixed priority order the buckets are declared in.

use super::{Group, GroupKey, Member};
use crate::session_mr::Bucket;

pub(super) fn by_mr(members: &[Member]) -> Vec<Group> {
    let mut buckets: Vec<(Bucket, Vec<usize>)> = Vec::new();
    for member in members {
        match buckets.iter_mut().find(|(bucket, _)| *bucket == member.mr) {
            Some((_, items)) => items.push(member.item),
            None => buckets.push((member.mr, vec![member.item])),
        }
    }
    buckets.sort_by_key(|(bucket, _)| *bucket);
    buckets
        .into_iter()
        .map(|(bucket, items)| Group {
            key: GroupKey::Mr(bucket),
            title: bucket.title().to_owned(),
            items,
        })
        .collect()
}
