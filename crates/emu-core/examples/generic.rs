//! Quick check of the generic table extractor against a live emulator.
//!
//! ```bash
//! HOME=$EMULATOR_HOME cargo run -p emu-core --example generic
//! ```

use emu_core::generic::{response_value, table_from_value};
use emu_core::{config, EmulatorClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = config::discover();
    let client = EmulatorClient::new(&config)?;

    let calls: Vec<(&str, &str, serde_json::Value)> = vec![
        ("dynamodb", "ListTables", serde_json::json!({})),
        ("sns", "ListTopics", serde_json::json!({})),
        ("iam", "ListUsers", serde_json::json!({})),
        ("ec2", "DescribeInstances", serde_json::json!({})),
        ("cloudformation", "ListStacks", serde_json::json!({})),
        ("logs", "DescribeLogGroups", serde_json::json!({})),
        ("lambda", "ListFunctions", serde_json::json!({})),
        ("sqs", "ListQueues", serde_json::json!({})),
        ("events", "ListEventBuses", serde_json::json!({})),
        ("stepfunctions", "ListStateMachines", serde_json::json!({})),
        ("ssm", "DescribeParameters", serde_json::json!({})),
        ("secretsmanager", "ListSecrets", serde_json::json!({})),
        ("kms", "ListKeys", serde_json::json!({})),
        ("ecr", "DescribeRepositories", serde_json::json!({})),
        ("ecs", "ListClusters", serde_json::json!({})),
    ];

    for (service, operation, input) in calls {
        match client.call(service, operation, input).await {
            Ok(response) => match response_value(&response) {
                Some(value) => {
                    let table = table_from_value(&value, operation);
                    println!(
                        "{service}.{operation}: {} cols, {} rows, token={:?}",
                        table.columns.len(),
                        table.rows.len(),
                        table.token.as_ref().map(|token| token.input_field.clone())
                    );
                    println!("  columns: {}", table.columns.join(" | "));
                    if let Some(row) = table.rows.first() {
                        println!("  first:   {}", row.cells.join(" | "));
                    }
                }
                None => println!("{service}.{operation}: no body"),
            },
            Err(error) => println!("{service}.{operation}: ERROR {}", error.message),
        }
    }

    Ok(())
}
