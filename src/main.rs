use std::collections::{HashSet};
use std::time::Instant;
use rand::RngExt;
use rand::seq::{IndexedRandom, SliceRandom};
use md5::{Md5, Digest};

struct BloomFilter {
    size_in_bits: usize,
    k: usize,
    bit_array: Vec<u8>,
}

impl BloomFilter {
    fn new(size_in_bits: usize, expected_elements: usize) -> Self {
        let k = if expected_elements == 0 {
            1
        } else {
            let k_f = (size_in_bits as f64 / expected_elements as f64) * std::f64::consts::LN_2;
            std::cmp::max(1, k_f.round() as usize)
        };
        let bytes_needed = (size_in_bits + 7) / 8;
        Self { size_in_bits, k, bit_array: vec![0; bytes_needed] }
    }

    fn get_hashes(&self, item: &[u8]) -> Vec<usize> {
        let mut hasher = Md5::new();
        hasher.update(item);
        let result = hasher.finalize();
        let mut h1_bytes = [0u8; 8];
        h1_bytes.copy_from_slice(&result[0..8]);
        let h1 = u64::from_le_bytes(h1_bytes) as usize;
        let mut h2_bytes = [0u8; 8];
        h2_bytes.copy_from_slice(&result[8..16]);
        let h2 = u64::from_le_bytes(h2_bytes) as usize;

        (0..self.k).map(|i| h1.wrapping_add(i.wrapping_mul(h2)) % self.size_in_bits).collect()
    }

    fn add(&mut self, item: &[u8]) {
        if self.size_in_bits > 0 {
            for h in self.get_hashes(item) {
                self.bit_array[h / 8] |= 1 << (h % 8);
            }
        }
    }

    fn lookup(&self, item: &[u8]) -> bool {
        if self.size_in_bits == 0 { return false; }
        for h in self.get_hashes(item) {
            if (self.bit_array[h / 8] & (1 << (h % 8))) == 0 { return false; }
        }
        true
    }
}

struct FastSkeleton {
    skeleton: Vec<u8>,
    head3: Vec<u32>,
    prev3: Vec<u32>,
    head2: Vec<u32>,
    has_byte: [bool; 256],
}

impl FastSkeleton {
    fn new(s: Vec<u8>) -> Self {
        let mut head3 = vec![u32::MAX; 1 << 24];
        let mut prev3 = vec![u32::MAX; s.len() + 1];
        let mut head2 = vec![u32::MAX; 1 << 16];
        let mut has_byte = [false; 256];

        for i in 0..s.len() {
            has_byte[s[i] as usize] = true;
            if i + 1 < s.len() {
                let h2 = ((s[i] as usize) << 8) | (s[i+1] as usize);
                head2[h2] = i as u32;
            }
            if i + 2 < s.len() {
                let h3 = ((s[i] as usize) << 16) | ((s[i+1] as usize) << 8) | (s[i+2] as usize);
                prev3[i] = head3[h3];
                head3[h3] = i as u32;
            }
        }
        Self { skeleton: s, head3, prev3, head2, has_byte }
    }

    fn find_longest_substring_of(&self, q: &[u8]) -> (usize, usize) {
        let mut best_start = 0;
        let mut max_l = 0;

        for i in 0..q.len() {
            if q.len() - i <= max_l { break; }

            if q.len() - i >= 3 {
                let h3 = ((q[i] as usize) << 16) | ((q[i+1] as usize) << 8) | (q[i+2] as usize);
                let mut curr = self.head3[h3];
                while curr != u32::MAX {
                    let s_idx = curr as usize;
                    let mut l = 3;
                    while i + l < q.len() && s_idx + l < self.skeleton.len() && self.skeleton[s_idx + l] == q[i + l] {
                        l += 1;
                    }
                    if l > max_l {
                        max_l = l;
                        best_start = i;
                    }
                    curr = self.prev3[s_idx];
                }
            } else if q.len() - i == 2 {
                if max_l < 2 {
                    let h2 = ((q[i] as usize) << 8) | (q[i+1] as usize);
                    if self.head2[h2] != u32::MAX {
                        max_l = 2;
                        best_start = i;
                    }
                }
            } else {
                if max_l < 1 {
                    if self.has_byte[q[i] as usize] {
                        max_l = 1;
                        best_start = i;
                    }
                }
            }
        }
        (best_start, max_l)
    }

    fn contains_exact(&self, q: &[u8]) -> bool {
        if q.is_empty() { return true; }
        if q.len() == 1 { return self.has_byte[q[0] as usize]; }
        if q.len() == 2 {
            let h2 = ((q[0] as usize) << 8) | (q[1] as usize);
            return self.head2[h2] != u32::MAX;
        }
        let h3 = ((q[0] as usize) << 16) | ((q[1] as usize) << 8) | (q[2] as usize);
        let mut curr = self.head3[h3];
        while curr != u32::MAX {
            let s_idx = curr as usize;
            let mut l = 3;
            while l < q.len() && s_idx + l < self.skeleton.len() && self.skeleton[s_idx + l] == q[l] {
                l += 1;
            }
            if l == q.len() { return true; }
            curr = self.prev3[s_idx];
        }
        false
    }
}

fn build_skeleton(stream: &[u8], min_match: usize) -> Vec<u8> {
    let mut s = Vec::with_capacity(stream.len());
    let mut head3 = vec![u32::MAX; 1 << 24];
    let mut prev3 = vec![u32::MAX; stream.len() + 1];

    let mut i = 0;
    while i < stream.len() {
        let mut max_l = 0;
        if i + 2 < stream.len() {
            let h3 = ((stream[i] as usize) << 16) | ((stream[i+1] as usize) << 8) | (stream[i+2] as usize);
            let mut curr = head3[h3];
            while curr != u32::MAX {
                let s_idx = curr as usize;
                let mut l = 3;
                while i + l < stream.len() && s_idx + l < s.len() && s[s_idx + l] == stream[i + l] {
                    l += 1;
                }
                if l > max_l { max_l = l; }
                curr = prev3[s_idx];
            }
        }

        if max_l >= min_match {
            i += max_l;
        } else {
            s.push(stream[i]);
            if s.len() >= 3 {
                let start = s.len() - 3;
                let h3 = ((s[start] as usize) << 16) | ((s[start+1] as usize) << 8) | (s[start+2] as usize);
                prev3[start] = head3[h3];
                head3[h3] = start as u32;
            }
            i += 1;
        }
    }
    s
}

fn lookup_greedy(q: &[u8], skel: &FastSkeleton, min_match: usize) -> bool {
    if q.is_empty() { return true; }
    if q.len() < min_match {
        return skel.contains_exact(q);
    }

    let (best_start, max_l) = skel.find_longest_substring_of(q);

    if max_l >= min_match {
        let left = &q[0..best_start];
        let right = &q[best_start + max_l ..];
        return lookup_greedy(left, skel, min_match) && lookup_greedy(right, skel, min_match);
    } else {
        return false;
    }
}

fn get_patterns(stream: &[u8], p: usize, target_pos: usize, target_neg: usize) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut valid_set = HashSet::new();
    for i in 0..=(stream.len() - p) {
        valid_set.insert(&stream[i..i+p]);
    }
    
    let mut pos_patterns = Vec::new();
    let mut valid_vec: Vec<_> = valid_set.iter().copied().collect();
    let mut rng = rand::rng();
    valid_vec.shuffle(&mut rng);
    for pat in valid_vec.into_iter().take(target_pos) {
        pos_patterns.push(pat.to_vec());
    }
    
    let mut neg_patterns = Vec::new();
    while neg_patterns.len() < target_neg {
        let mut pat = vec![0u8; p];
        rng.fill(&mut pat[..]);
        if !valid_set.contains(&pat[..]) {
            neg_patterns.push(pat);
        }
    }
    
    (pos_patterns, neg_patterns)
}

fn run_experiment(stream_name: &str, stream_bytes: &[u8], caps: &[usize], p: usize, base_min_match: usize) {
    println!("\n--- {} ---", stream_name);
    println!("| Cap (KB) | min_match | skeleton_size | bloom_size | skel_FPR | bloom_FPR | skel_FNR | bloom_FNR |");
    println!("|----------|-----------|---------------|------------|----------|-----------|----------|-----------|");

    let (pos_patterns, neg_patterns) = get_patterns(stream_bytes, p, 10000, 10000);
    let expected_elements = stream_bytes.len() - p + 1;

    for &cap in caps {
        let cap_bytes = cap * 1024;
        let mut best_m = base_min_match;
        let mut best_s = Vec::new();
        
        for m in (base_min_match..=32).rev() {
            let s = build_skeleton(stream_bytes, m);
            if s.len() <= cap_bytes {
                best_m = m;
                best_s = s;
                break;
            }
        }
        
        if best_s.is_empty() {
            best_s = build_skeleton(stream_bytes, base_min_match);
            best_m = base_min_match;
        }

        let skeleton_size_bytes = best_s.len();
        let skel = FastSkeleton::new(best_s);

        let mut bloom = BloomFilter::new(cap_bytes * 8, expected_elements);
        for i in 0..=(stream_bytes.len() - p) {
            bloom.add(&stream_bytes[i..i+p]);
        }

        let mut skel_fp = 0;
        for pat in &neg_patterns {
            if lookup_greedy(pat, &skel, best_m) { skel_fp += 1; }
        }

        let mut skel_fn = 0;
        for pat in &pos_patterns {
            if !lookup_greedy(pat, &skel, best_m) { skel_fn += 1; }
        }

        let mut bloom_fp = 0;
        for pat in &neg_patterns {
            if bloom.lookup(pat) { bloom_fp += 1; }
        }

        let mut bloom_fn = 0;
        for pat in &pos_patterns {
            if !bloom.lookup(pat) { bloom_fn += 1; }
        }

        println!(
            "| {:<8} | {:<9} | {:.2}KB       | {:.2}KB     | {:.4}   | {:.4}     | {:.4}   | {:.4}     |",
            cap, best_m,
            skeleton_size_bytes as f64 / 1024.0,
            (bloom.bit_array.len() as f64) / 1024.0,
            skel_fp as f64 / neg_patterns.len() as f64,
            bloom_fp as f64 / neg_patterns.len() as f64,
            skel_fn as f64 / pos_patterns.len() as f64,
            bloom_fn as f64 / pos_patterns.len() as f64
        );
    }
}

fn main() {
    let mut rng = rand::rng();
    let min_match = 4;
    let p = 16;

    // 1. RANDOM STREAM (Incompressible)
    let mut random_stream = vec![0u8; 1024 * 1024];
    rng.fill(&mut random_stream[..]);
    run_experiment("INCOMPRESSIBLE STREAM (1MB Random)", &random_stream, &[50, 100, 500, 1000], p, min_match);

    // 2. REDUNDANT STREAM (Compressible)
    let mut dict = Vec::new();
    for _ in 0..100 {
        let mut block = vec![0u8; 50];
        rng.fill(&mut block[..]);
        dict.push(block);
    }
    let mut redundant_stream = Vec::with_capacity(1024 * 1024);
    while redundant_stream.len() < 1024 * 1024 {
        let block = dict.choose(&mut rng).unwrap();
        let remaining = 1024 * 1024 - redundant_stream.len();
        if remaining < block.len() {
            redundant_stream.extend_from_slice(&block[..remaining]);
        } else {
            redundant_stream.extend_from_slice(block);
        }
    }
    run_experiment("COMPRESSIBLE STREAM (1MB Redundant)", &redundant_stream, &[2, 5, 10], p, min_match);
}
