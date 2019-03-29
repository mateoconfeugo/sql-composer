pub mod direct;

pub mod mysql;
pub mod postgres;
pub mod rusqlite;

pub use super::parser::parse_template;
use crate::types::{ParsedItem, Sql, SqlComposition, SqlCompositionAlias, SqlDbObject};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Default)]
pub struct ComposerConfig {
    start: usize,
}

pub trait Composer: Sized {
    type Value: Copy;

    fn compose(&self, s: &SqlComposition) -> (String, Vec<Self::Value>) {
        let item = ParsedItem::generated(s.clone(), None).unwrap();

        self.compose_statement(&item, 1usize, false)
    }

    fn compose_statement(
        &self,
        sc: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> (String, Vec<Self::Value>) {
        let mut i = offset;

        let mut sql = String::new();

        let mut values: Vec<Self::Value> = vec![];

        if sc.item.command.is_some() {
            return self.compose_command(&sc, i, true).unwrap();
        }

        let mut pad = true;
        let mut skip_this = false;
        let mut skip_next = false;

        for c in &sc.item.sql {
            pad = true;
            skip_this = skip_next;
            skip_next = false;

            let (sub_sql, sub_values) = match c {
                Sql::Literal(t) => (t.to_string(), vec![]),
                Sql::Binding(b) => self.bind_values(b.item.name.to_string(), i),
                Sql::Composition((ss, _aliases)) => self.compose_statement(&ss, i, true),
                Sql::Ending(e) => {
                    pad = false;

                    if child {
                        ("".to_string(), vec![])
                    }
                    else {
                        (e.to_string(), vec![])
                    }
                }
                Sql::DbObject(dbo) => {
                    let dbo_alias = SqlCompositionAlias::DbObject(
                        SqlDbObject::new(dbo.item.object_name.to_string(), None).unwrap(),
                    );

                    if let Some(mv) = self.mock_values().get(&dbo_alias) {
                        let (mock_sql, mock_values) = self.mock_compose(mv, i);

                        //TODO: this should call the alias function on dbo_alias, which uses
                        //object_alias but falls back to object_name
                        let mock_sql = format!("( {} ) AS {}", mock_sql, dbo.item.object_name);

                        (mock_sql, mock_values)
                    }
                    else {
                        (dbo.to_string(), vec![])
                    }
                }
                Sql::Keyword(k) => (k.to_string(), vec![]),
            };

            if sub_sql.len() == 0 {
                continue;
            }

            if sub_sql == "," {
                skip_this = true;
            }

            if !skip_this && pad && sql.len() > 0 {
                sql.push(' ');
            }

            sql.push_str(&sub_sql);

            for sv in sub_values {
                values.push(sv);
            }

            i = values.len() + offset;
        }

        (sql, values)
    }

    fn compose_command<'c>(
        &self,
        composition: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> Result<(String, Vec<Self::Value>), ()> {
        match &composition.item.command {
            Some(s) => {
                match s.item().to_lowercase().as_str() {
                    "compose" => {
                        let mut out = composition.clone();

                        out.item.command = None;

                        match &out.item.of[0].item().path() {
                            Some(path) => match self
                                .mock_values()
                                .get(&SqlCompositionAlias::Path(path.into()))
                            {
                                Some(e) => Ok(self.mock_compose(e, offset)),
                                None => Ok(self.compose_statement(
                                    &out.item.aliases.get(&out.item.of[0].item()).unwrap(),
                                    offset,
                                    child,
                                )),
                            },
                            None => Ok(self.compose_statement(
                                &out.item.aliases.get(&out.item.of[0].item()).unwrap(),
                                offset,
                                child,
                            )),
                        }
                    }
                    "count" => self.compose_count_command(composition, offset, child),
                    "union" => self.compose_union_command(composition, offset, child),
                    // TODO: handle this error better
                    _ => panic!("unknown call"),
                }
            }
            None => Ok(self.compose_statement(&composition, offset, child)),
        }
    }

    fn compose_count_command(
        &self,
        composition: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> Result<(String, Vec<Self::Value>), ()>;

    fn compose_count_default_command(
        &self,
        composition: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> Result<(String, Vec<Self::Value>), ()> {
        let mut out = SqlComposition::default();

        let mut select = String::from("SELECT COUNT(");

        let columns = composition.item.column_list().unwrap();

        if let Some(c) = columns {
            select.push_str(&c);
        }
        else {
            select.push('1');
        }

        select.push_str(") FROM ");

        out.push_generated_literal(&select, Some("COUNT".into()))
            .unwrap();

        for position in composition.item.of.iter() {
            out.push_generated_literal("(", Some("COUNT".into()))
                .unwrap();
            match composition.item.aliases.get(&position.item()) {
                Some(sc) => {
                    out.push_sub_comp(sc.clone()).unwrap();
                }
                None => {
                    panic!("no position found with position: {:?}", position);
                }
            }

            out.push_generated_literal(") AS count_main", Some("COUNT".into()))
                .unwrap();
        }

        out.push_generated_end(Some("COUNT".into())).unwrap();

        let item = ParsedItem::generated(out, Some("COUNT".into())).unwrap();

        Ok(self.compose_statement(&item, offset, child))
    }

    fn compose_union_command(
        &self,
        composition: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> Result<(String, Vec<Self::Value>), ()>;

    fn compose_union_default_command(
        &self,
        composition: &ParsedItem<SqlComposition>,
        offset: usize,
        child: bool,
    ) -> Result<(String, Vec<Self::Value>), ()> {
        let mut out = SqlComposition::default();

        // columns in this case would mean an compose on each side of the union literal
        let _columns = composition.item.column_list().unwrap();

        let mut i = 0usize;

        if composition.item.of.len() < 2 {
            panic!("union requires 2 of arguments");
        }

        for position in composition.item.of.iter() {
            if i > 0 {
                out.push_generated_literal("UNION ", Some("UNION".into()))
                    .unwrap();
            }

            match composition.item.aliases.get(&position.item()) {
                Some(sc) => {
                    out.push_sub_comp(sc.clone()).unwrap();
                }
                None => {
                    panic!("no alias found with alias: {:?}", position.item());
                }
            }

            i += 1;
        }

        out.push_generated_end(Some("UNION".into())).unwrap();

        let item = ParsedItem::generated(out, Some("UNION".into())).unwrap();

        Ok(self.compose_statement(&item, offset, child))
    }

    fn bind_var_tag(&self, u: usize, name: String) -> String;

    fn bind_values(&self, name: String, offset: usize) -> (String, Vec<Self::Value>);

    fn get_values(&self, name: String) -> Option<&Vec<Self::Value>>;

    fn insert_value(&mut self, name: String, values: Vec<Self::Value>) -> ();

    fn config() -> ComposerConfig;

    //fn insert_mock_values(&mut self, alias: SqlCompositionAlias, values: Vec<Self::Value>) -> ();

    fn root_mock_values(&self) -> &Vec<BTreeMap<String, Self::Value>>;

    fn mock_values(&self) -> &HashMap<SqlCompositionAlias, Vec<BTreeMap<String, Self::Value>>>;

    fn mock_compose(
        &self,
        mock_values: &Vec<BTreeMap<String, Self::Value>>,
        offset: usize,
    ) -> (String, Vec<Self::Value>) {
        let mut sql = String::new();
        let mut values: Vec<Self::Value> = vec![];

        let mut i = offset;
        let mut r = 0;
        let mut c = 0;

        if i == 0 {
            i = 1
        }

        let mut expected_columns: Option<u8> = None;

        if mock_values.is_empty() {
            panic!("mock_values cannot be empty");
        }
        else {
            for row in mock_values.iter() {
                if r > 0 {
                    sql.push_str(" UNION ALL ");
                }

                sql.push_str("SELECT ");

                for (name, value) in row {
                    c += 1;

                    if c > 1 {
                        sql.push_str(", ")
                    }

                    sql.push_str(&self.bind_var_tag(i, name.to_string()));
                    sql.push_str(&format!(" AS {}", &name));

                    values.push(*value);

                    i += 1;
                }

                if let Some(ec) = expected_columns {
                    if c != ec {
                        panic!("expected {} columns found {} for row {}", ec, c, r);
                    }
                }
                else {
                    expected_columns = Some(c);
                }

                r += 1;
                c = 0;
            }
        }

        (sql, values)
    }
}
