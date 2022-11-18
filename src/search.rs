use crate::chain;
use crate::file_io;
use crate::params::*;
use crate::screen;
use crate::types::*;
use log::*;
use rayon::prelude::*;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

pub fn search(command_params: CommandParams) {
    let now = Instant::now();
    let mut ref_marker_files = vec![];
    for file in command_params.ref_files.iter() {
        if !file.contains(".sketch") && !file.contains(".marker") {
            warn!(
                "{} does not have .sketch or .marker as an extension; skipping file",
                file
            );
        } else if file.contains(".marker") {
            ref_marker_files.push(file.clone());
        }
    }

    if ref_marker_files.len() == 0 {
        error!("No sketch files found in the folder. Sketch files must be generated by `skani sketch` and have the .sketch extension.");
        std::process::exit(1)
    }

    let ref_sketches;
    let sketch_params;
    (sketch_params, ref_sketches) = file_io::sketches_from_sketch(&ref_marker_files, true);
    let screen_val;
    if command_params.screen_val == 0. {
        if sketch_params.use_aa {
            screen_val = SEARCH_AAI_CUTOFF_DEFAULT;
        } else {
            screen_val = SEARCH_ANI_CUTOFF_DEFAULT;
        }
    } else {
        screen_val = command_params.screen_val;
    }

    info!("Loading markers time: {}", now.elapsed().as_secs_f32());
    let kmer_to_sketch;
    if command_params.screen {
        let now = Instant::now();
        info!("Full index option detected; generating marker hash table");
        kmer_to_sketch = screen::kmer_to_sketch_from_refs(&ref_sketches);
        info!("Full indexing time: {}", now.elapsed().as_secs_f32());
    } else {
        kmer_to_sketch = KmerToSketch::default();
    }

    let now = Instant::now();
    assert!(ref_sketches.len() == ref_marker_files.len());
    let anis: Mutex<Vec<AniEstResult>> = Mutex::new(vec![]);
    let folder = Path::new(&ref_marker_files[0]).parent().unwrap();
    for (count, query_file) in command_params.query_files.iter().enumerate() {
        let query_params;
        let query_sketches;
        if command_params.queries_are_sketch {
            (query_params, query_sketches) =
                file_io::sketches_from_sketch(&vec![query_file.clone()], false);
            if query_params != sketch_params {
                panic!("Query sketch parameters not equal to reference sketch parameters. Exiting");
            }
        } else {
            if command_params.individual_contig_q {
                query_sketches = file_io::fastx_to_multiple_sketch_rewrite(
                    &vec![query_file.clone()],
                    &sketch_params,
                    true,
                );
            } else {
                query_sketches =
                    file_io::fastx_to_sketches(&vec![query_file.clone()], &sketch_params, true);
            }
        }

        if !query_sketches.is_empty() {
            let query_sketch = &query_sketches[0];
            let js = 0..ref_marker_files.len();
            let refs_to_try;
            if !command_params.screen {
                let refs_to_try_mutex: Mutex<Vec<&String>> = Mutex::new(vec![]);
                js.into_par_iter().for_each(|j| {
                    let ref_sketch = &ref_sketches[j];
                    if chain::check_markers_quickly(&query_sketch, ref_sketch, screen_val) {
                        let mut lock = refs_to_try_mutex.lock().unwrap();
                        lock.push(&ref_sketches[j].file_name);
                    }
                });
                refs_to_try = refs_to_try_mutex.into_inner().unwrap();
            } else {
                refs_to_try = screen::screen_refs_filenames(
                    screen_val,
                    &kmer_to_sketch,
                    query_sketch,
                    &sketch_params,
                    &ref_sketches,
                );
            }
            debug!("Refs to try {}", refs_to_try.len());
            let js = 0..refs_to_try.len();
            js.into_par_iter().for_each(|j| {
                let original_file = &refs_to_try[j];
                let sketch_file = folder.join(
                    Path::new(&format!("{}.sketch", original_file))
                        .file_name()
                        .unwrap(),
                );
                let (sketch_params_ref, ref_sketch) = file_io::sketches_from_sketch(
                    &vec![sketch_file.to_str().unwrap().to_string()],
                    false,
                );
                let map_params = chain::map_params_from_sketch(
                    &ref_sketch[0],
                    sketch_params_ref.use_aa,
                    &command_params,
                );
                let ani_res;
                if map_params != MapParams::default() {
                    ani_res = chain::chain_seeds(&ref_sketch[0], &query_sketch, map_params);
                } else {
                    ani_res = AniEstResult::default();
                }
                if ani_res.ani > 0.5 {
                    let mut locked = anis.lock().unwrap();
                    locked.push(ani_res);
                }
            });
        }

        if count % 100 == 0 && count != 0 {
            info!("{} query sequences processed.", count);
        }
    }
    let anis = anis.into_inner().unwrap();
    file_io::write_query_ref_list(
        &anis,
        &command_params.out_file_name,
        command_params.max_results,
        sketch_params.use_aa,
    );
    info!("Searching time: {}", now.elapsed().as_secs_f32());
}
