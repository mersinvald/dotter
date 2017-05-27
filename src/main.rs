#[macro_use]
extern crate clap;
extern crate yaml_rust;

use std::env;
use std::process;
use std::io::Read;
use std::fs::File;

use yaml_rust::{Yaml, YamlLoader};

macro_rules! verb {
    ( $verbosity:expr, $level:expr, $( $message:expr ),* ) => {
        if $verbosity >= $level {
            println!($($message),*);
        }
    };
}

fn main() {
    // Parse arguments
    let matches = get_args();
    // Do the "implies" relation between verbose and dry_run
    let act = matches.occurrences_of("dry_run") == 0;
    let verbosity = matches.occurrences_of("verbose");
    // If dry run, then at least one verbosity level.
    let verbosity = if act {
            verbosity
        } else {
            std::cmp::max(1, verbosity)
        };


    // Change dir
    let dir = matches.value_of("directory").unwrap();
    verb!(verbosity, 1, "Changing directory to {}", dir);
    if env::set_current_dir(dir).is_err() {
        println!("Error: No such directory {}", dir);
        process::exit(1);
    }

    verb!(verbosity, 3, "{:?}", matches);

    // Execute subcommand
    if let Some(_) = matches.subcommand_matches("deploy") {
        deploy(&matches, verbosity, act);
    } else if let Some(_) = matches.subcommand_matches("config") {
        config(&matches, verbosity, act);
    } else {
        unreachable!();
    }
}

fn load_file(filename: &str) -> Yaml {
    if let Ok(mut file) = File::open(filename) {
        let mut buf = String::new();
        file.read_to_string(&mut buf)
            .expect("Failed to read from file");
        YamlLoader::load_from_str(&buf)
            .expect("Failed to parse config")
            .swap_remove(0)
    } else {
        // No file
        Yaml::Null
    }
}

fn deploy(matches: &clap::ArgMatches<'static>,
          verbosity: u64, act: bool) {
    verb!(verbosity, 3, "Deploy args: {:?}", matches);
    let filename = matches.value_of("config").unwrap();
    let configuration = load_file(filename);
    verb!(verbosity, 2, "configuration: {:?}", configuration);
}

fn config(matches: &clap::ArgMatches<'static>,
          verbosity: u64, act: bool) {
    verb!(verbosity, 3, "Config args: {:?}", matches);
}

fn get_args() -> clap::ArgMatches<'static> {
    clap::App::new("Dotter")
        .setting(clap::AppSettings::SubcommandRequiredElseHelp)
        .version("1.0.0")
        .author(crate_authors!())
        .about("A small dotfile manager.")
        .arg(clap::Arg::with_name("directory")
             .short("d")
             .long("directory")
             .value_name("DIRECTORY")
             .takes_value(true)
             .default_value(".")
             .help("Do all operations relative to this directory."))
        .arg(clap::Arg::with_name("config")
             .short("c")
             .long("config")
             .value_name("CONFIG")
             .takes_value(true)
             .default_value("dotter.yml")
             .help("Config file for dotter."))
        .arg(clap::Arg::with_name("secrets")
             .short("s")
             .long("secrets")
             .value_name("SECRETS")
             .takes_value(true)
             .default_value("secrets.yml")
             .help("Secrets file for dotter, doesn't have to exist."))
        .arg(clap::Arg::with_name("verbose")
             .short("v")
             .long("verbose")
             .multiple(true)
             .help("Print information about what's being done. Repeat for \
                   more information."))
        .arg(clap::Arg::with_name("dry_run")
             .long("dry-run")
             .help("Dry run - don't do anything, only print information. \
                   Implies -v at least once."))
        .subcommand(clap::SubCommand::with_name("deploy")
                    .about("Copy all files to their configured locations.")
                    .arg(clap::Arg::with_name("nocache")
                         .short("c")
                         .long("nocache")
                         .help("Create a directory with templated files, \
                               then copy from there."))
                    .arg(clap::Arg::with_name("cache_directory")
                         .short("d")
                         .long("cache-directory")
                         .value_name("DIRECTORY")
                         .takes_value(true)
                         .default_value("dotter_cache")
                         .help("Directory to cache in.")))
        .subcommand(clap::SubCommand::with_name("config")
                    .about("Configure files/variables.")
                    .arg(clap::Arg::with_name("file")
                         .short("f")
                         .long("file")
                         .help("Operate on files."))
                    .arg(clap::Arg::with_name("variable")
                         .short("v")
                         .long("variable")
                         .help("Operate on variables."))
                    .arg(clap::Arg::with_name("secret")
                         .short("s")
                         .long("secret")
                         .help("Operate on secrets."))
                    .group(clap::ArgGroup::with_name("target")
                           .required(true)
                           .args(&["file", "variable", "secret"]))
                    .arg(clap::Arg::with_name("add")
                         .short("a")
                         .long("add")
                         .value_names(&["from", "to"])
                         .help("In case of file, add file -> target entry, \
                               in case of variable/secret, \
                               add key -> value entry."))
                    .arg(clap::Arg::with_name("remove")
                         .short("r")
                         .long("remove")
                         .value_name("object")
                         .takes_value(true)
                         .help("Remove a file or variable from configuration."))
                    .arg(clap::Arg::with_name("display")
                         .short("d")
                         .long("display")
                         .help("Display the configuration."))
                    .group(clap::ArgGroup::with_name("action")
                           .required(true)
                           .args(&["add", "remove", "display"])))
        .get_matches()
}
