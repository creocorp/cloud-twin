#[derive(Debug, Clone)]
pub struct DynamoTable {
    pub name:                  String,
    pub key_schema:            String, // JSON
    pub attribute_definitions: String, // JSON
    pub created_at:            String,
}

#[derive(Debug, Clone)]
pub struct DynamoItem {
    pub table_name: String,
    pub pk:         String,
    pub sk:         String,
    pub item:       String, // JSON
    pub created_at: String,
}
