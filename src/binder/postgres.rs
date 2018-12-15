use std::collections::HashMap;

use postgres::types::ToSql;

use ::parser::{SqlStatement, parse_template};
use super::{Binder, BinderConfig};

struct PostgresBinder<'a> {
    config: BinderConfig,
    values: HashMap<String, Vec<&'a ToSql>>
}

impl<'a> PostgresBinder<'a> {
    fn new() -> Self {
        Self{
         config: Self::config(),
         values: HashMap::new()
        }
    }

}

impl <'a>Binder for PostgresBinder<'a> {
    type Value = &'a (dyn ToSql + 'a);

    fn config() -> BinderConfig {
        BinderConfig {
            start: 0
        }
    }

    fn bind_var_tag(&self, u: usize, _name: String) -> String {
        format!("${}", u)
    }

    fn bind_values(&self, name: String, offset: usize) -> (String, Vec<Self::Value>) {
        let mut sql = String::new();
        let mut new_values = vec![];

        let i = offset;

        match self.values.get(&name) {
            Some(v) => {
                for iv in v.iter() {
                    if new_values.len() > 0 {
                        sql.push_str(", ");
                    }

                    sql.push_str(&self.bind_var_tag(new_values.len() + offset, name.to_string()));

                    new_values.push(*iv);
                }
            },
            None => panic!("no value for binding: {}", new_values.len())
        };

        (sql, new_values)
    }

    fn get_values(&self, name: String) -> Option<&Vec<Self::Value>> {
        self.values.get(&name)
    }

    fn insert_value(&mut self, name: String, values: Vec<Self::Value>) -> () {
        self.values.insert(name, values);
    }
}

#[cfg(test)]
mod tests {
    use super::{Binder, PostgresBinder};

    use ::parser::{SqlStatement, parse_template};

    use postgres::{Connection, TlsMode};
    use postgres::rows::{Row, Rows};
    use postgres::types::ToSql;

    use std::collections::BTreeMap;

    #[derive(Debug, PartialEq)]
    struct Person {
        id: i32,
        name: String,
        data: Option<Vec<u8>>,
    }

    fn setup_db() -> Connection {
        Connection::connect("postgres://vagrant:vagrant@localhost:5432", TlsMode::None).unwrap()
    }

    #[test]
    fn test_binding() {
        let conn = setup_db();

        conn.execute("DROP TABLE IF EXISTS person;", &[]).unwrap();

        conn.execute("CREATE TABLE IF NOT EXISTS person (
                        id              SERIAL PRIMARY KEY,
                        name            VARCHAR NOT NULL,
                        data            BYTEA
                      )", &[]).unwrap();


        let person = Person {
            id: 0,
            name: "Steven".to_string(),
            data: None,
        };

        let (remaining, insert_stmt) = parse_template(b"INSERT INTO person (name, data) VALUES (:name:, :data:);").unwrap();

        assert_eq!(remaining, b"", "insert stmt nothing remaining");

        let mut bv = PostgresBinder::new();

        bv.values.insert("name".into(), vec![&person.name]);
        bv.values.insert("data".into(), vec![&person.data]);

        let (bound_sql, bindings) = bv.bind(insert_stmt);

        let expected_bound_sql = "INSERT INTO person (name, data) VALUES ($1, $2);";

        assert_eq!(bound_sql, expected_bound_sql, "insert basic bindings");

        conn.execute(
            &bound_sql,
            &bindings,
        ).unwrap();

        let (remaining, select_stmt) = parse_template(b"SELECT id, name, data FROM person WHERE name = ':name:' AND name = ':name:';").unwrap();

        assert_eq!(remaining, b"", "select stmt nothing remaining");

        let (bound_sql, bindings) = bv.bind(select_stmt);

        let expected_bound_sql = "SELECT id, name, data FROM person WHERE name = $1 AND name = $2;";

        assert_eq!(bound_sql, expected_bound_sql, "select multi-use bindings");

        let stmt = conn.prepare(&bound_sql).unwrap();

        let mut people:Vec<Person> = vec![];

        for row in &stmt.query(&bindings).unwrap() {
            people.push(Person {
                id: row.get(0),
                name: row.get(1),
                data: row.get(2),
            });
        }

        assert_eq!(people.len(), 1, "found 1 person");
        let found = &people[0];

        assert_eq!(found.name, person.name, "person's name");
        assert_eq!(found.data, person.data, "person's data");
    }

    fn get_row_values(row: Row) -> Vec<String> {
        (0..4).fold(Vec::new(), |mut acc, i| {
            acc.push(row.get(i));
            acc
        })
    }

    #[test]
    fn test_bind_simple_template() {
        let conn = setup_db();

        let stmt = SqlStatement::from_utf8_path_name(b"src/tests/values/simple.tql").unwrap();

        let mut binder = PostgresBinder::new();

        binder.values.insert("a".into(), vec![&"a_value"]);
        binder.values.insert("b".into(), vec![&"b_value"]);
        binder.values.insert("c".into(), vec![&"c_value"]);
        binder.values.insert("d".into(), vec![&"d_value"]);

        let mut mock_values:Vec<BTreeMap<std::string::String, &dyn ToSql>> = vec![BTreeMap::new()];

        mock_values[0].insert("col_1".into(), &"a_value");
        mock_values[0].insert("col_2".into(), &"b_value");
        mock_values[0].insert("col_3".into(), &"c_value");
        mock_values[0].insert("col_4".into(), &"d_value");

        let (bound_sql, bindings) = binder.bind(stmt);
        let (mut mock_bound_sql, mock_bindings) = binder.mock_bind(mock_values, 0);

        mock_bound_sql.push(';');

        let mut prep_stmt = conn.prepare(&bound_sql).unwrap();


        let mut values:Vec<Vec<String>> = vec![];
        let mut mock_values:Vec<Vec<String>> = vec![];

        for row in &prep_stmt.query(&bindings).unwrap() {
            values.push(get_row_values(row));
        }

        let mut mock_prep_stmt = conn.prepare(&mock_bound_sql).unwrap();

        for row in &mock_prep_stmt.query(&mock_bindings).unwrap() {
            mock_values.push(get_row_values(row));
        }

        assert_eq!(bound_sql, mock_bound_sql, "preparable statements match");
        assert_eq!(values, mock_values, "exected values");
    }

    #[test]
    fn test_bind_include_template() {
        let conn = setup_db();

        let stmt = SqlStatement::from_utf8_path_name(b"src/tests/values/include.tql").unwrap();

        let mut binder = PostgresBinder::new();

        binder.values.insert("a".into(), vec![&"a_value"]);
        binder.values.insert("b".into(), vec![&"b_value"]);
        binder.values.insert("c".into(), vec![&"c_value"]);
        binder.values.insert("d".into(), vec![&"d_value"]);
        binder.values.insert("e".into(), vec![&"e_value"]);

        let mut mock_values:Vec<BTreeMap<std::string::String, &dyn ToSql>> = vec![];

        mock_values.push(BTreeMap::new());
        mock_values[0].insert("col_1".into(), &"e_value");
        mock_values[0].insert("col_2".into(), &"d_value");
        mock_values[0].insert("col_3".into(), &"b_value");
        mock_values[0].insert("col_4".into(), &"a_value");

        mock_values.push(BTreeMap::new());
        mock_values[1].insert("col_1".into(), &"a_value");
        mock_values[1].insert("col_2".into(), &"b_value");
        mock_values[1].insert("col_3".into(), &"c_value");
        mock_values[1].insert("col_4".into(), &"d_value");

        let (bound_sql, bindings) = binder.bind(stmt);
        let (mut mock_bound_sql, mock_bindings) = binder.mock_bind(mock_values, 0);

        mock_bound_sql.push(';');

        println!("bound_sql: {}", bound_sql);

        let mut prep_stmt = conn.prepare(&bound_sql).unwrap();

        let mut values:Vec<Vec<String>> = vec![];

        for row in &prep_stmt.query(&bindings).unwrap() {
            values.push(get_row_values(row));
        }

        let mut mock_prep_stmt = conn.prepare(&bound_sql).unwrap();

        let mut mock_values:Vec<Vec<String>> = vec![];

        for row in &mock_prep_stmt.query(&mock_bindings).unwrap() {
            mock_values.push(get_row_values(row));
        }

        assert_eq!(bound_sql, mock_bound_sql, "preparable statements match");
        assert_eq!(values, mock_values, "exected values");
    }

    #[test]
    fn test_bind_double_include_template() {
        let conn = setup_db();

        let stmt = SqlStatement::from_utf8_path_name(b"src/tests/values/double-include.tql").unwrap();

        let mut binder = PostgresBinder::new();

        binder.values.insert("a".into(), vec![&"a_value"]);
        binder.values.insert("b".into(), vec![&"b_value"]);
        binder.values.insert("c".into(), vec![&"c_value"]);
        binder.values.insert("d".into(), vec![&"d_value"]);
        binder.values.insert("e".into(), vec![&"e_value"]);
        binder.values.insert("f".into(), vec![&"f_value"]);

        let mut mock_values:Vec<BTreeMap<std::string::String, &dyn ToSql>> = vec![];

        mock_values.push(BTreeMap::new());
        mock_values[0].insert("col_1".into(), &"d_value");
        mock_values[0].insert("col_2".into(), &"f_value");
        mock_values[0].insert("col_3".into(), &"b_value");
        mock_values[0].insert("col_4".into(), &"a_value");

        mock_values.push(BTreeMap::new());
        mock_values[1].insert("col_1".into(), &"e_value");
        mock_values[1].insert("col_2".into(), &"d_value");
        mock_values[1].insert("col_3".into(), &"b_value");
        mock_values[1].insert("col_4".into(), &"a_value");

        mock_values.push(BTreeMap::new());
        mock_values[2].insert("col_1".into(), &"a_value");
        mock_values[2].insert("col_2".into(), &"b_value");
        mock_values[2].insert("col_3".into(), &"c_value");
        mock_values[2].insert("col_4".into(), &"d_value");

        let (bound_sql, bindings) = binder.bind(stmt);
        let (mut mock_bound_sql, mock_bindings) = binder.mock_bind(mock_values, 0);

        mock_bound_sql.push(';');

        let mut prep_stmt = conn.prepare(&bound_sql).unwrap();

        let mut values:Vec<Vec<String>> = vec![];

        for row in &prep_stmt.query(&bindings).unwrap() {
            values.push(get_row_values(row));
        }

        let mut mock_prep_stmt = conn.prepare(&bound_sql).unwrap();

        let mut mock_values:Vec<Vec<String>> = vec![];

        for row in &mock_prep_stmt.query(&mock_bindings).unwrap() {
            mock_values.push(get_row_values(row));
        }

        assert_eq!(bound_sql, mock_bound_sql, "preparable statements match");
        assert_eq!(values, mock_values, "exected values");
    }

    #[test]
    fn test_multi_value_bind() {
        let conn = setup_db();

        let (remaining, stmt) = parse_template(b"SELECT col_1, col_2, col_3, col_4 FROM (::src/tests/values/double-include.tql::) AS main WHERE col_1 in (:col_1_values:) AND col_3 IN (:col_3_values:);").unwrap();

        let expected_sql = "SELECT col_1, col_2, col_3, col_4 FROM (SELECT $1 AS col_1, $2 AS col_2, $3 AS col_3, $4 AS col_4 UNION SELECT $5 AS col_1, $6 AS col_2, $7 AS col_3, $8 AS col_4 UNION SELECT $9 AS col_1, $10 AS col_2, $11 AS col_3, $12 AS col_4) AS main WHERE col_1 in ($13, $14) AND col_3 IN ($15, $16);";

        let expected_values = vec![
            vec!["a_value", "b_value", "c_value", "d_value"],
            vec!["d_value", "f_value", "b_value", "a_value"],
        ];

        println!("setup binder");
        let mut binder = PostgresBinder::new();

        binder.values.insert("a".into(), vec![&"a_value"]);
        binder.values.insert("b".into(), vec![&"b_value"]);
        binder.values.insert("c".into(), vec![&"c_value"]);
        binder.values.insert("d".into(), vec![&"d_value"]);
        binder.values.insert("e".into(), vec![&"e_value"]);
        binder.values.insert("f".into(), vec![&"f_value"]);
        binder.values.insert("col_1_values".into(), vec![&"d_value", &"a_value"]);
        binder.values.insert("col_3_values".into(), vec![&"b_value", &"c_value"]);

        println!("binding");
        let (bound_sql, bindings) = binder.bind(stmt);

        println!("bound_sql: {}", bound_sql);

        let mut prep_stmt = conn.prepare(&bound_sql).unwrap();

        let mut values:Vec<Vec<String>> = vec![];

        for row in &prep_stmt.query(&bindings).unwrap() {
            values.push(get_row_values(row));
        }

        assert_eq!(bound_sql, expected_sql, "preparable statements match");
        assert_eq!(values, expected_values, "exected values");
    }
}
