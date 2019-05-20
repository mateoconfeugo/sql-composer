extern crate rusqlite;

use quicli::prelude::*;
use structopt::StructOpt;

// use sql_composer::composer::rusqlite::RusqliteComposer;
use rusqlite::types::{ValueRef};
use rusqlite::Connection;
pub use rusqlite::types::{Null, ToSql};
use sql_composer::composer::{ComposerBuilder, ComposerConnection};
use sql_composer::types::{SqlComposition, value::Value};
use std::collections::{BTreeMap, HashMap};

use sql_composer::parser::bind_value_named_set;
use sql_composer::types::{CompleteStr, Span};

#[derive(Debug, StructOpt)]
struct QueryArgs {
    #[structopt(flatten)]
    verbosity: Verbosity,
    /// Uri to the database
    #[structopt(long = "uri", short = "u")]
    uri: String,
    /// Path to the template
    #[structopt(long = "path", short = "p")]
    path: String,
    /// a comma seperated list of key:value pairs
    #[structopt(long = "bind", short = "b")]
    bind: Option<String>,
    /// values to use in place of a path, made up of a comma seperated list of [] containing key:value pairs
    #[structopt(long = "mock-path")]
    mock_path: Vec<String>,
    /// values to use in place of a table, made up of a comma seperated list of [] containing key:value pairs
    #[structopt(long = "mock-table")]
    mock_table: Vec<String>,
}

#[derive(Debug, StructOpt)]
struct ParseArgs {
    #[structopt(flatten)]
    verbosity: Verbosity,
    /// Uri to the database
    #[structopt(long = "uri", short = "u")]
    uri: String,
    /// Path to the template
    #[structopt(long = "path", short = "p")]
    path: String,
}

#[derive(Debug, StructOpt)]
enum Cli {
    #[structopt(name = "query")]
    Query(QueryArgs),
}

/*
../target/debug/sqlc query --uri sqlite://:memory: --path /vol/projects/sql-composer/sql-composer/src/tests/values/double-include.tql --bind "[a: ['a_value'], b: ['b_value'], c: ['c_value'], d: ['d_value'], e: ['e_value'], f: ['f_value']]" -vvv
*/
fn main() -> CliResult {
    let args = Cli::from_args();

    match args {
        Cli::Query(r) => query(r),
    }
}

fn setup(verbosity: Verbosity) -> CliResult {
    verbosity
        .setup_env_logger(&env!("CARGO_PKG_NAME"))
        .expect("unable to setup evn_logger");

    Ok(())
}

fn parse(args: QueryArgs) -> CliResult {
    setup(args.verbosity)?;

    let comp = SqlComposition::from_str(&args.path);

    Ok(())
}

fn query(args: QueryArgs) -> CliResult {
    setup(args.verbosity)?;

    let parsed_comp = SqlComposition::from_path_name(&args.path).unwrap();
    let comp = parsed_comp.item;

    let uri = args.uri;

    let mut builder = ComposerBuilder::default();

    builder.uri(&uri);

    if uri.starts_with("sqlite://") {
            //TODO: base off of uri
            let conn = match uri.as_str() {
                "sqlite://:memory:" => Connection::open_in_memory().unwrap(),
                _ => unimplemented!("not currently passing uri correctly")
            };

            let mut parsed_values: BTreeMap<String, Vec<Value>> = BTreeMap::new();
            let mut values:BTreeMap<String, Vec<&ToSql>> = BTreeMap::new();

            if let Some(b) = args.bind {
              let (remaining, bvns) = bind_value_named_set(Span::new(CompleteStr(&b))).unwrap();

              parsed_values = bvns;
            }

            values = parsed_values.iter().fold(BTreeMap::new(), |mut acc, (k, v)| {
                let entry = acc.entry(k.to_string()).or_insert(vec![]);
                *entry = v.iter().map(|x| x as &ToSql).collect();

                acc
            });

            let (mut prep_stmt, bindings) = conn.compose(&comp, values, vec![], HashMap::new()).unwrap();

            let driver_rows = prep_stmt
                .query_map(&bindings, |driver_row| {
                    (0..driver_row.column_count()).fold(
                        Ok(vec![]),
                        |acc: Result<Vec<String>, rusqlite::Error>, i| {
                            if let Ok(mut acc) = acc {
                                let raw = driver_row.get_raw(i);

                                acc.push(
                                    match raw {
                                        ValueRef::Null => "NULL".to_string(),
                                        ValueRef::Integer(int) => int.to_string(),
                                        ValueRef::Real(r) => r.to_string(),
                                        ValueRef::Text(t) => t.to_string(),
                                        ValueRef::Blob(vc) => {
                                            std::string::String::from_utf8(vc.to_vec()).unwrap()
                                        }
                                    }
                                    .into(),
                                );

                                Ok(acc)
                            }
                            else {
                                acc
                            }
                        },
                    )
                })
                .unwrap();

            let mut rows = vec![];

            for driver_row in driver_rows {
                rows.push(driver_row);
            }

            for row in rows {
                println!("row: {:?}", row);
            }

    }
    else if uri.starts_with("postgres://") {
        unimplemented!("postgres cli query in progress");
    }
    else {
        panic!("unknown uri type: {}", uri);
    }

    Ok(())
}
