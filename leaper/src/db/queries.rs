use std::sync::Arc;

use surrealdb::RecordId;

use crate::db::{DB, DBResult};

pub trait DBQuery {
    type Output;

    const QUERY_STR: &'static str;

    async fn execute(self, db: Arc<DB>) -> DBResult<Self::Output>;
}

#[derive(bon::Builder, macros::DBQuery)]
#[query(check)]
pub struct RelateQuery {
    #[builder(into)]
    #[var(sql = "RELATE {}->")]
    in_: RecordId,
    #[builder(into)]
    #[var(sql = "{}->")]
    table: surrealdb::sql::Table,
    #[builder(into)]
    #[var(sql = "{}")]
    out: RecordId,
}

#[derive(bon::Builder, macros::DBQuery)]
#[query(output = "Option<RecordId>")]
pub struct CreateEmptyIdQuery {
    #[builder(into)]
    #[var(sql = "(CREATE {}).id")]
    table: surrealdb::sql::Table,
}
