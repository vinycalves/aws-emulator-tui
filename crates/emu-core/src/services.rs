//! Curated resource views.
//!
//! The catalog lets the console drive every service. This module adds the part
//! that makes the TUI feel like a console rather than a request debugger: for a
//! set of services, a table of live resources with create, inspect and delete
//! actions.

use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::client::{ApiError, ApiResponse, EmulatorClient};
use crate::sigv4;
use crate::xml;

/// One browsable resource collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewId {
    S3Buckets,
    S3Objects(String),
    DynamoDbTables,
    SqsQueues,
    SnsTopics,
    LambdaFunctions,
    IamUsers,
    IamRoles,
    Ec2Instances,
    CfnStacks,
    LogGroups,
    Secrets,
    SsmParameters,
    EventBuses,
    EcsClusters,
    EcrRepositories,
    KmsKeys,
    StateMachines,
}

impl ViewId {
    pub fn service_key(&self) -> &'static str {
        match self {
            ViewId::S3Buckets | ViewId::S3Objects(_) => "s3",
            ViewId::DynamoDbTables => "dynamodb",
            ViewId::SqsQueues => "sqs",
            ViewId::SnsTopics => "sns",
            ViewId::LambdaFunctions => "lambda",
            ViewId::IamUsers | ViewId::IamRoles => "iam",
            ViewId::Ec2Instances => "ec2",
            ViewId::CfnStacks => "cloudformation",
            ViewId::LogGroups => "logs",
            ViewId::Secrets => "secretsmanager",
            ViewId::SsmParameters => "ssm",
            ViewId::EventBuses => "events",
            ViewId::EcsClusters => "ecs",
            ViewId::EcrRepositories => "ecr",
            ViewId::KmsKeys => "kms",
            ViewId::StateMachines => "stepfunctions",
        }
    }

    pub fn title(&self) -> String {
        match self {
            ViewId::S3Buckets => "Buckets".to_string(),
            ViewId::S3Objects(bucket) => format!("Objects in {bucket}"),
            ViewId::DynamoDbTables => "Tables".to_string(),
            ViewId::SqsQueues => "Queues".to_string(),
            ViewId::SnsTopics => "Topics".to_string(),
            ViewId::LambdaFunctions => "Functions".to_string(),
            ViewId::IamUsers => "Users".to_string(),
            ViewId::IamRoles => "Roles".to_string(),
            ViewId::Ec2Instances => "Instances".to_string(),
            ViewId::CfnStacks => "Stacks".to_string(),
            ViewId::LogGroups => "Log groups".to_string(),
            ViewId::Secrets => "Secrets".to_string(),
            ViewId::SsmParameters => "Parameters".to_string(),
            ViewId::EventBuses => "Event buses".to_string(),
            ViewId::EcsClusters => "Clusters".to_string(),
            ViewId::EcrRepositories => "Repositories".to_string(),
            ViewId::KmsKeys => "Keys".to_string(),
            ViewId::StateMachines => "State machines".to_string(),
        }
    }

    pub fn columns(&self) -> &'static [&'static str] {
        match self {
            ViewId::S3Buckets => &["NAME", "CREATED"],
            ViewId::S3Objects(_) => &["KEY", "SIZE", "LAST MODIFIED"],
            ViewId::DynamoDbTables => &["TABLE"],
            ViewId::SqsQueues => &["QUEUE URL"],
            ViewId::SnsTopics => &["TOPIC", "ARN"],
            ViewId::LambdaFunctions => &["FUNCTION", "RUNTIME", "MEMORY", "MODIFIED"],
            ViewId::IamUsers => &["USER", "USER ID", "CREATED"],
            ViewId::IamRoles => &["ROLE", "ARN", "CREATED"],
            ViewId::Ec2Instances => &["INSTANCE", "TYPE", "STATE", "PRIVATE IP", "NAME"],
            ViewId::CfnStacks => &["STACK", "STATUS", "CREATED"],
            ViewId::LogGroups => &["LOG GROUP", "STORED", "CREATED"],
            ViewId::Secrets => &["SECRET", "DESCRIPTION", "LAST CHANGED"],
            ViewId::SsmParameters => &["PARAMETER", "TYPE", "VERSION", "MODIFIED"],
            ViewId::EventBuses => &["BUS", "ARN"],
            ViewId::EcsClusters => &["CLUSTER", "ARN"],
            ViewId::EcrRepositories => &["REPOSITORY", "URI", "CREATED"],
            ViewId::KmsKeys => &["KEY ID", "ARN"],
            ViewId::StateMachines => &["STATE MACHINE", "TYPE", "CREATED"],
        }
    }

    pub fn parent(&self) -> Option<ViewId> {
        match self {
            ViewId::S3Objects(_) => Some(ViewId::S3Buckets),
            _ => None,
        }
    }
}

/// One row in a resource table.
#[derive(Clone, Debug)]
pub struct Row {
    /// Stable identifier used by inspect/delete actions.
    pub id: String,
    pub cells: Vec<String>,
    /// Structured detail when the list call already returned it.
    pub detail: Option<Value>,
}

/// A fetched resource table.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub columns: Vec<&'static str>,
    pub rows: Vec<Row>,
}

/// A curated service in the sidebar.
#[derive(Clone, Copy, Debug)]
pub struct CuratedService {
    pub key: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    pub category: &'static str,
}

/// The services with a first-class resource browser.
pub const CURATED: &[CuratedService] = &[
    CuratedService {
        key: "s3",
        title: "S3",
        subtitle: "Buckets and objects",
        category: "Storage",
    },
    CuratedService {
        key: "dynamodb",
        title: "DynamoDB",
        subtitle: "Tables",
        category: "Databases",
    },
    CuratedService {
        key: "sqs",
        title: "SQS",
        subtitle: "Queues",
        category: "Messaging",
    },
    CuratedService {
        key: "sns",
        title: "SNS",
        subtitle: "Topics",
        category: "Messaging",
    },
    CuratedService {
        key: "events",
        title: "EventBridge",
        subtitle: "Event buses",
        category: "Messaging",
    },
    CuratedService {
        key: "lambda",
        title: "Lambda",
        subtitle: "Functions",
        category: "Compute",
    },
    CuratedService {
        key: "ec2",
        title: "EC2",
        subtitle: "Instances",
        category: "Compute",
    },
    CuratedService {
        key: "ecs",
        title: "ECS",
        subtitle: "Clusters",
        category: "Containers",
    },
    CuratedService {
        key: "ecr",
        title: "ECR",
        subtitle: "Repositories",
        category: "Containers",
    },
    CuratedService {
        key: "iam",
        title: "IAM",
        subtitle: "Users and roles",
        category: "Security",
    },
    CuratedService {
        key: "kms",
        title: "KMS",
        subtitle: "Keys",
        category: "Security",
    },
    CuratedService {
        key: "secretsmanager",
        title: "Secrets Manager",
        subtitle: "Secrets",
        category: "Security",
    },
    CuratedService {
        key: "ssm",
        title: "SSM",
        subtitle: "Parameters",
        category: "Management",
    },
    CuratedService {
        key: "cloudformation",
        title: "CloudFormation",
        subtitle: "Stacks",
        category: "Management",
    },
    CuratedService {
        key: "logs",
        title: "CloudWatch Logs",
        subtitle: "Log groups",
        category: "Management",
    },
    CuratedService {
        key: "stepfunctions",
        title: "Step Functions",
        subtitle: "State machines",
        category: "Orchestration",
    },
];

/// The default view for a curated service key.
pub fn base_view(key: &str) -> Option<ViewId> {
    Some(match key {
        "s3" => ViewId::S3Buckets,
        "dynamodb" => ViewId::DynamoDbTables,
        "sqs" => ViewId::SqsQueues,
        "sns" => ViewId::SnsTopics,
        "lambda" => ViewId::LambdaFunctions,
        "ec2" => ViewId::Ec2Instances,
        "ecs" => ViewId::EcsClusters,
        "ecr" => ViewId::EcrRepositories,
        "iam" => ViewId::IamUsers,
        "kms" => ViewId::KmsKeys,
        "secretsmanager" => ViewId::Secrets,
        "ssm" => ViewId::SsmParameters,
        "cloudformation" => ViewId::CfnStacks,
        "logs" => ViewId::LogGroups,
        "events" => ViewId::EventBuses,
        "stepfunctions" => ViewId::StateMachines,
        _ => return None,
    })
}

/// The service and operation behind a list view, for request history labels.
pub fn list_operation(view: &ViewId) -> (&'static str, &'static str) {
    match view {
        ViewId::S3Buckets => ("s3", "ListBuckets"),
        ViewId::S3Objects(_) => ("s3", "ListObjectsV2"),
        ViewId::DynamoDbTables => ("dynamodb", "ListTables"),
        ViewId::SqsQueues => ("sqs", "ListQueues"),
        ViewId::SnsTopics => ("sns", "ListTopics"),
        ViewId::LambdaFunctions => ("lambda", "ListFunctions"),
        ViewId::IamUsers => ("iam", "ListUsers"),
        ViewId::IamRoles => ("iam", "ListRoles"),
        ViewId::Ec2Instances => ("ec2", "DescribeInstances"),
        ViewId::CfnStacks => ("cloudformation", "ListStacks"),
        ViewId::LogGroups => ("logs", "DescribeLogGroups"),
        ViewId::Secrets => ("secretsmanager", "ListSecrets"),
        ViewId::SsmParameters => ("ssm", "DescribeParameters"),
        ViewId::EventBuses => ("events", "ListEventBuses"),
        ViewId::EcsClusters => ("ecs", "ListClusters"),
        ViewId::EcrRepositories => ("ecr", "DescribeRepositories"),
        ViewId::KmsKeys => ("kms", "ListKeys"),
        ViewId::StateMachines => ("stepfunctions", "ListStateMachines"),
    }
}

/// Fetch a resource table.
pub async fn fetch(client: &EmulatorClient, view: &ViewId) -> Result<Table, ApiError> {
    let (columns, rows) = match view {
        ViewId::S3Buckets => {
            let response = client.s3("GET", "/", "", Vec::new()).await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("S3 ListBuckets did not return XML"))?;
            let buckets = xml::path(document.root_element(), &["Buckets", "Bucket"]);
            let rows = buckets
                .into_iter()
                .map(|bucket| Row {
                    id: xml::field(bucket, "Name"),
                    cells: vec![
                        xml::field(bucket, "Name"),
                        xml::field(bucket, "CreationDate"),
                    ],
                    detail: Some(xml::node_to_json(bucket)),
                })
                .collect();
            (ViewId::S3Buckets.columns(), rows)
        }
        ViewId::S3Objects(bucket) => {
            let path = format!("/{}", sigv4::encode(bucket));
            let response = client.s3("GET", &path, "list-type=2", Vec::new()).await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("S3 ListObjectsV2 did not return XML"))?;
            let objects = xml::path(document.root_element(), &["Contents"]);
            let rows = objects
                .into_iter()
                .map(|object| {
                    let key = xml::field(object, "Key");
                    Row {
                        id: key.clone(),
                        cells: vec![
                            key,
                            xml::field(object, "Size"),
                            xml::field(object, "LastModified"),
                        ],
                        detail: Some(xml::node_to_json(object)),
                    }
                })
                .collect();
            (ViewId::S3Objects(String::new()).columns(), rows)
        }
        ViewId::DynamoDbTables => {
            let response = client.call("dynamodb", "ListTables", json!({})).await?;
            let rows = json_array(&response, "TableNames")
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .map(|name| Row {
                    id: name.clone(),
                    cells: vec![name],
                    detail: None,
                })
                .collect();
            (ViewId::DynamoDbTables.columns(), rows)
        }
        ViewId::SqsQueues => {
            let response = client.call("sqs", "ListQueues", json!({})).await?;
            let rows = json_array(&response, "QueueUrls")
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .map(|url| Row {
                    id: url.clone(),
                    cells: vec![url],
                    detail: None,
                })
                .collect();
            (ViewId::SqsQueues.columns(), rows)
        }
        ViewId::SnsTopics => {
            let response = client.call("sns", "ListTopics", json!({})).await?;
            let rows = json_array(&response, "Topics")
                .iter()
                .map(|topic| {
                    let arn = get_str(topic, "TopicArn");
                    let name = arn.rsplit(':').next().unwrap_or(&arn).to_string();
                    Row {
                        id: arn.clone(),
                        cells: vec![name, arn.clone()],
                        detail: Some(topic.clone()),
                    }
                })
                .collect();
            (ViewId::SnsTopics.columns(), rows)
        }
        ViewId::LambdaFunctions => {
            let response = client.call("lambda", "ListFunctions", json!({})).await?;
            let rows = json_array(&response, "Functions")
                .iter()
                .map(|function| {
                    let name = get_str(function, "FunctionName");
                    Row {
                        id: name.clone(),
                        cells: vec![
                            name,
                            get_str(function, "Runtime"),
                            get_str(function, "MemorySize"),
                            get_str(function, "LastModified"),
                        ],
                        detail: Some(function.clone()),
                    }
                })
                .collect();
            (ViewId::LambdaFunctions.columns(), rows)
        }
        ViewId::IamUsers => {
            let response = client.call("iam", "ListUsers", json!({})).await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("IAM did not return XML"))?;
            let nodes = xml::path(
                document.root_element(),
                &["ListUsersResult", "Users", "member"],
            );
            let rows = nodes
                .into_iter()
                .map(|user| Row {
                    id: xml::field(user, "UserName"),
                    cells: vec![
                        xml::field(user, "UserName"),
                        xml::field(user, "UserId"),
                        xml::field(user, "CreateDate"),
                    ],
                    detail: Some(xml::node_to_json(user)),
                })
                .collect();
            (ViewId::IamUsers.columns(), rows)
        }
        ViewId::IamRoles => {
            let response = client.call("iam", "ListRoles", json!({})).await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("IAM did not return XML"))?;
            let nodes = xml::path(
                document.root_element(),
                &["ListRolesResult", "Roles", "member"],
            );
            let rows = nodes
                .into_iter()
                .map(|role| Row {
                    id: xml::field(role, "RoleName"),
                    cells: vec![
                        xml::field(role, "RoleName"),
                        xml::field(role, "Arn"),
                        xml::field(role, "CreateDate"),
                    ],
                    detail: Some(xml::node_to_json(role)),
                })
                .collect();
            (ViewId::IamRoles.columns(), rows)
        }
        ViewId::Ec2Instances => {
            let response = client.call("ec2", "DescribeInstances", json!({})).await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("EC2 did not return XML"))?;
            let nodes = xml::path(
                document.root_element(),
                &["reservationSet", "item", "instancesSet", "item"],
            );
            let rows = nodes
                .into_iter()
                .map(|instance| {
                    let state = xml::child(instance, "instanceState")
                        .map(|node| xml::field(node, "name"))
                        .unwrap_or_default();
                    let name = xml::child(instance, "tagSet")
                        .and_then(|tags| {
                            xml::children(tags, "item")
                                .into_iter()
                                .find(|tag| xml::field(*tag, "key").eq_ignore_ascii_case("Name"))
                        })
                        .map(|tag| xml::field(tag, "value"))
                        .unwrap_or_default();
                    Row {
                        id: xml::field(instance, "instanceId"),
                        cells: vec![
                            xml::field(instance, "instanceId"),
                            xml::field(instance, "instanceType"),
                            state,
                            xml::field(instance, "privateIpAddress"),
                            name,
                        ],
                        detail: Some(xml::node_to_json(instance)),
                    }
                })
                .collect();
            (ViewId::Ec2Instances.columns(), rows)
        }
        ViewId::CfnStacks => {
            let response = client
                .call("cloudformation", "ListStacks", json!({}))
                .await?;
            let document = xml::parse(&response.body)
                .ok_or_else(|| ApiError::transport("CloudFormation did not return XML"))?;
            let nodes = xml::path(
                document.root_element(),
                &["ListStacksResult", "StackSummaries", "member"],
            );
            let rows = nodes
                .into_iter()
                .map(|stack| Row {
                    id: xml::field(stack, "StackName"),
                    cells: vec![
                        xml::field(stack, "StackName"),
                        xml::field(stack, "StackStatus"),
                        xml::field(stack, "CreationTime"),
                    ],
                    detail: Some(xml::node_to_json(stack)),
                })
                .collect();
            (ViewId::CfnStacks.columns(), rows)
        }
        ViewId::LogGroups => {
            let response = client.call("logs", "DescribeLogGroups", json!({})).await?;
            let rows = json_array(&response, "logGroups")
                .iter()
                .map(|group| {
                    let name = get_str(group, "logGroupName");
                    Row {
                        id: name.clone(),
                        cells: vec![
                            name,
                            human_bytes(group.get("storedBytes").and_then(Value::as_u64)),
                            group
                                .get("creationTime")
                                .and_then(Value::as_i64)
                                .map(format_epoch_ms)
                                .unwrap_or_default(),
                        ],
                        detail: Some(group.clone()),
                    }
                })
                .collect();
            (ViewId::LogGroups.columns(), rows)
        }
        ViewId::Secrets => {
            let response = client
                .call("secretsmanager", "ListSecrets", json!({}))
                .await?;
            let rows = json_array(&response, "SecretList")
                .iter()
                .map(|secret| {
                    let name = get_str(secret, "Name");
                    Row {
                        id: name.clone(),
                        cells: vec![
                            name,
                            get_str(secret, "Description"),
                            timestamp_value(secret.get("LastChangedDate")),
                        ],
                        detail: Some(secret.clone()),
                    }
                })
                .collect();
            (ViewId::Secrets.columns(), rows)
        }
        ViewId::SsmParameters => {
            let response = client.call("ssm", "DescribeParameters", json!({})).await?;
            let rows = json_array(&response, "Parameters")
                .iter()
                .map(|parameter| {
                    let name = get_str(parameter, "Name");
                    Row {
                        id: name.clone(),
                        cells: vec![
                            name,
                            get_str(parameter, "Type"),
                            get_str(parameter, "Version"),
                            get_str(parameter, "LastModifiedDate"),
                        ],
                        detail: Some(parameter.clone()),
                    }
                })
                .collect();
            (ViewId::SsmParameters.columns(), rows)
        }
        ViewId::EventBuses => {
            let response = client.call("events", "ListEventBuses", json!({})).await?;
            let rows = json_array(&response, "EventBuses")
                .iter()
                .map(|bus| {
                    let name = get_str(bus, "Name");
                    Row {
                        id: name.clone(),
                        cells: vec![name, get_str(bus, "Arn")],
                        detail: Some(bus.clone()),
                    }
                })
                .collect();
            (ViewId::EventBuses.columns(), rows)
        }
        ViewId::EcsClusters => {
            let response = client.call("ecs", "ListClusters", json!({})).await?;
            let rows = json_array(&response, "clusterArns")
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .map(|arn| Row {
                    id: arn.clone(),
                    cells: vec![
                        arn.rsplit('/').next().unwrap_or(&arn).to_string(),
                        arn.clone(),
                    ],
                    detail: None,
                })
                .collect();
            (ViewId::EcsClusters.columns(), rows)
        }
        ViewId::EcrRepositories => {
            let response = client
                .call("ecr", "DescribeRepositories", json!({}))
                .await?;
            let rows = json_array(&response, "repositories")
                .iter()
                .map(|repository| {
                    let name = get_str(repository, "repositoryName");
                    Row {
                        id: name.clone(),
                        cells: vec![
                            name,
                            get_str(repository, "repositoryUri"),
                            timestamp_value(repository.get("createdAt")),
                        ],
                        detail: Some(repository.clone()),
                    }
                })
                .collect();
            (ViewId::EcrRepositories.columns(), rows)
        }
        ViewId::KmsKeys => {
            let response = client.call("kms", "ListKeys", json!({})).await?;
            let rows = json_array(&response, "Keys")
                .iter()
                .map(|key| {
                    let id = get_str(key, "KeyId");
                    Row {
                        id: id.clone(),
                        cells: vec![id, get_str(key, "KeyArn")],
                        detail: Some(key.clone()),
                    }
                })
                .collect();
            (ViewId::KmsKeys.columns(), rows)
        }
        ViewId::StateMachines => {
            let response = client
                .call("stepfunctions", "ListStateMachines", json!({}))
                .await?;
            let rows = json_array(&response, "stateMachines")
                .iter()
                .map(|machine| {
                    let arn = get_str(machine, "stateMachineArn");
                    Row {
                        id: arn.clone(),
                        cells: vec![
                            get_str(machine, "name"),
                            get_str(machine, "type"),
                            timestamp_value(machine.get("creationDate")),
                        ],
                        detail: Some(machine.clone()),
                    }
                })
                .collect();
            (ViewId::StateMachines.columns(), rows)
        }
    };

    Ok(Table {
        columns: columns.to_vec(),
        rows,
    })
}

/// The call that opens a detail view for a row, when one exists.
pub fn describe_call(view: &ViewId, id: &str) -> Option<(&'static str, &'static str, Value)> {
    Some(match view {
        ViewId::DynamoDbTables => ("dynamodb", "DescribeTable", json!({"TableName": id})),
        ViewId::SqsQueues => (
            "sqs",
            "GetQueueAttributes",
            json!({"QueueUrl": id, "AttributeNames": ["All"]}),
        ),
        ViewId::SnsTopics => ("sns", "GetTopicAttributes", json!({"TopicArn": id})),
        ViewId::LambdaFunctions => ("lambda", "GetFunction", json!({"FunctionName": id})),
        ViewId::IamUsers => ("iam", "GetUser", json!({"UserName": id})),
        ViewId::IamRoles => ("iam", "GetRole", json!({"RoleName": id})),
        ViewId::Ec2Instances => ("ec2", "DescribeInstances", json!({"InstanceIds": [id]})),
        ViewId::CfnStacks => ("cloudformation", "DescribeStacks", json!({"StackName": id})),
        ViewId::LogGroups => (
            "logs",
            "DescribeLogGroups",
            json!({"logGroupNamePrefix": id}),
        ),
        ViewId::Secrets => ("secretsmanager", "DescribeSecret", json!({"SecretId": id})),
        ViewId::SsmParameters => (
            "ssm",
            "GetParameter",
            json!({"Name": id, "WithDecryption": true}),
        ),
        ViewId::EventBuses => ("events", "DescribeEventBus", json!({"Name": id})),
        ViewId::EcsClusters => ("ecs", "DescribeClusters", json!({"clusters": [id]})),
        ViewId::EcrRepositories => (
            "ecr",
            "DescribeRepositories",
            json!({"repositoryNames": [id]}),
        ),
        ViewId::KmsKeys => ("kms", "DescribeKey", json!({"KeyId": id})),
        ViewId::StateMachines => (
            "stepfunctions",
            "DescribeStateMachine",
            json!({"stateMachineArn": id}),
        ),
        ViewId::S3Buckets | ViewId::S3Objects(_) => return None,
    })
}

/// How the create action collects its input.
#[derive(Clone, Debug)]
pub enum CreateKind {
    /// A single name prompt.
    Name {
        title: &'static str,
        placeholder: &'static str,
    },
    /// A JSON document, pre-filled with the required fields.
    Json {
        title: &'static str,
        template: Value,
    },
}

/// The create form for a view, if creating is supported.
pub fn create_kind(view: &ViewId) -> Option<CreateKind> {
    Some(match view {
        ViewId::S3Buckets => CreateKind::Name {
            title: "Create bucket",
            placeholder: "bucket-name",
        },
        ViewId::DynamoDbTables => CreateKind::Json {
            title: "Create table",
            template: json!({
                "TableName": "",
                "AttributeDefinitions": [{"AttributeName": "id", "AttributeType": "S"}],
                "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}],
                "BillingMode": "PAY_PER_REQUEST"
            }),
        },
        ViewId::SqsQueues => CreateKind::Name {
            title: "Create queue",
            placeholder: "queue-name",
        },
        ViewId::SnsTopics => CreateKind::Name {
            title: "Create topic",
            placeholder: "topic-name",
        },
        ViewId::LogGroups => CreateKind::Name {
            title: "Create log group",
            placeholder: "/aws/my-group",
        },
        ViewId::Secrets => CreateKind::Json {
            title: "Create secret",
            template: json!({"Name": "", "SecretString": ""}),
        },
        ViewId::SsmParameters => CreateKind::Json {
            title: "Put parameter",
            template: json!({"Name": "", "Type": "String", "Value": ""}),
        },
        ViewId::EventBuses => CreateKind::Name {
            title: "Create event bus",
            placeholder: "bus-name",
        },
        ViewId::EcsClusters => CreateKind::Name {
            title: "Create cluster",
            placeholder: "cluster-name",
        },
        ViewId::EcrRepositories => CreateKind::Name {
            title: "Create repository",
            placeholder: "repository-name",
        },
        ViewId::KmsKeys => CreateKind::Json {
            title: "Create key",
            template: json!({"Description": ""}),
        },
        ViewId::StateMachines => CreateKind::Json {
            title: "Create state machine",
            template: json!({
                "name": "",
                "definition": "{\"StartAt\":\"Done\",\"States\":{\"Done\":{\"Type\":\"Succeed\"}}}",
                "roleArn": "arn:aws:iam::000000000000:role/emulator"
            }),
        },
        _ => return None,
    })
}

/// A write action: a catalog call, or a raw S3 request.
#[derive(Clone, Debug)]
pub enum WriteAction {
    Call {
        service: &'static str,
        operation: &'static str,
        input: Value,
    },
    S3 {
        method: &'static str,
        path: String,
        query: &'static str,
        body: Vec<u8>,
    },
}

/// Build a create action from the form input.
pub fn build_create(view: &ViewId, input: Value) -> Result<WriteAction, String> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    let action = match view {
        ViewId::S3Buckets => {
            require_name(&name)?;
            WriteAction::S3 {
                method: "PUT",
                path: format!("/{}", sigv4::encode(&name)),
                query: "",
                body: Vec::new(),
            }
        }
        ViewId::SqsQueues => {
            require_name(&name)?;
            WriteAction::Call {
                service: "sqs",
                operation: "CreateQueue",
                input: json!({"QueueName": name}),
            }
        }
        ViewId::SnsTopics => {
            require_name(&name)?;
            WriteAction::Call {
                service: "sns",
                operation: "CreateTopic",
                input: json!({"Name": name}),
            }
        }
        ViewId::LogGroups => {
            require_name(&name)?;
            WriteAction::Call {
                service: "logs",
                operation: "CreateLogGroup",
                input: json!({"logGroupName": name}),
            }
        }
        ViewId::EventBuses => {
            require_name(&name)?;
            WriteAction::Call {
                service: "events",
                operation: "CreateEventBus",
                input: json!({"Name": name}),
            }
        }
        ViewId::EcsClusters => {
            require_name(&name)?;
            WriteAction::Call {
                service: "ecs",
                operation: "CreateCluster",
                input: json!({"clusterName": name}),
            }
        }
        ViewId::EcrRepositories => {
            require_name(&name)?;
            WriteAction::Call {
                service: "ecr",
                operation: "CreateRepository",
                input: json!({"repositoryName": name}),
            }
        }
        ViewId::DynamoDbTables => WriteAction::Call {
            service: "dynamodb",
            operation: "CreateTable",
            input,
        },
        ViewId::Secrets => WriteAction::Call {
            service: "secretsmanager",
            operation: "CreateSecret",
            input,
        },
        ViewId::SsmParameters => WriteAction::Call {
            service: "ssm",
            operation: "PutParameter",
            input,
        },
        ViewId::KmsKeys => WriteAction::Call {
            service: "kms",
            operation: "CreateKey",
            input,
        },
        ViewId::StateMachines => WriteAction::Call {
            service: "stepfunctions",
            operation: "CreateStateMachine",
            input,
        },
        _ => return Err("this view does not support create".to_string()),
    };
    Ok(action)
}

/// Build a delete action for one row.
pub fn build_delete(view: &ViewId, id: &str) -> Option<WriteAction> {
    Some(match view {
        ViewId::S3Buckets => WriteAction::S3 {
            method: "DELETE",
            path: format!("/{}", sigv4::encode(id)),
            query: "",
            body: Vec::new(),
        },
        ViewId::S3Objects(bucket) => WriteAction::S3 {
            method: "DELETE",
            path: format!("/{}/{}", sigv4::encode(bucket), sigv4::encode_path(id)),
            query: "",
            body: Vec::new(),
        },
        ViewId::DynamoDbTables => WriteAction::Call {
            service: "dynamodb",
            operation: "DeleteTable",
            input: json!({"TableName": id}),
        },
        ViewId::SqsQueues => WriteAction::Call {
            service: "sqs",
            operation: "DeleteQueue",
            input: json!({"QueueUrl": id}),
        },
        ViewId::SnsTopics => WriteAction::Call {
            service: "sns",
            operation: "DeleteTopic",
            input: json!({"TopicArn": id}),
        },
        ViewId::LambdaFunctions => WriteAction::Call {
            service: "lambda",
            operation: "DeleteFunction",
            input: json!({"FunctionName": id}),
        },
        ViewId::IamUsers => WriteAction::Call {
            service: "iam",
            operation: "DeleteUser",
            input: json!({"UserName": id}),
        },
        ViewId::IamRoles => WriteAction::Call {
            service: "iam",
            operation: "DeleteRole",
            input: json!({"RoleName": id}),
        },
        ViewId::CfnStacks => WriteAction::Call {
            service: "cloudformation",
            operation: "DeleteStack",
            input: json!({"StackName": id}),
        },
        ViewId::LogGroups => WriteAction::Call {
            service: "logs",
            operation: "DeleteLogGroup",
            input: json!({"logGroupName": id}),
        },
        ViewId::Secrets => WriteAction::Call {
            service: "secretsmanager",
            operation: "DeleteSecret",
            input: json!({"SecretId": id, "ForceDeleteWithoutRecovery": true}),
        },
        ViewId::SsmParameters => WriteAction::Call {
            service: "ssm",
            operation: "DeleteParameter",
            input: json!({"Name": id}),
        },
        ViewId::EventBuses => WriteAction::Call {
            service: "events",
            operation: "DeleteEventBus",
            input: json!({"Name": id}),
        },
        ViewId::EcsClusters => WriteAction::Call {
            service: "ecs",
            operation: "DeleteCluster",
            input: json!({"cluster": id}),
        },
        ViewId::EcrRepositories => WriteAction::Call {
            service: "ecr",
            operation: "DeleteRepository",
            input: json!({"repositoryName": id, "force": true}),
        },
        ViewId::KmsKeys => WriteAction::Call {
            service: "kms",
            operation: "ScheduleKeyDeletion",
            input: json!({"KeyId": id, "PendingWindowInDays": 7}),
        },
        ViewId::StateMachines => WriteAction::Call {
            service: "stepfunctions",
            operation: "DeleteStateMachine",
            input: json!({"stateMachineArn": id}),
        },
        ViewId::Ec2Instances => return None,
    })
}

/// Execute a write action.
pub async fn execute(
    client: &EmulatorClient,
    action: &WriteAction,
) -> Result<ApiResponse, ApiError> {
    match action {
        WriteAction::Call {
            service,
            operation,
            input,
        } => client.call(service, operation, input.clone()).await,
        WriteAction::S3 {
            method,
            path,
            query,
            body,
        } => client.s3(method, path, query, body.clone()).await,
    }
}

/// Probe a curated service by loading its default view.
pub async fn probe(client: &EmulatorClient, key: &str) -> Result<(), ApiError> {
    let view = base_view(key)
        .ok_or_else(|| ApiError::transport(format!("no curated view for `{key}`")))?;
    fetch(client, &view).await.map(|_| ())
}

/// Read the caller identity (`sts:GetCallerIdentity`).
pub async fn caller_identity(client: &EmulatorClient) -> Result<String, ApiError> {
    let response = client.call("sts", "GetCallerIdentity", json!({})).await?;
    let document =
        xml::parse(&response.body).ok_or_else(|| ApiError::transport("STS did not return XML"))?;
    let root = document.root_element();
    let result = xml::child(root, "GetCallerIdentityResult").unwrap_or(root);
    let account = xml::field(result, "Account");
    let arn = xml::field(result, "Arn");
    if account.is_empty() && arn.is_empty() {
        Ok("identity unavailable".to_string())
    } else {
        Ok(format!("{account} · {arn}"))
    }
}

fn require_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        Err("name must not be empty".to_string())
    } else {
        Ok(())
    }
}

fn json_array(response: &ApiResponse, key: &str) -> Vec<Value> {
    response
        .json
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn get_str(value: &Value, key: &str) -> String {
    value
        .get(key)
        .map(|value| match value {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn human_bytes(bytes: Option<u64>) -> String {
    let Some(bytes) = bytes else {
        return String::new();
    };
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn timestamp_value(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => {
            let Some(seconds) = number.as_f64() else {
                return String::new();
            };
            let milliseconds = if seconds > 1_000_000_000_000.0 {
                seconds
            } else {
                seconds * 1000.0
            };
            format_epoch_ms(milliseconds as i64)
        }
        _ => String::new(),
    }
}

fn format_epoch_ms(milliseconds: i64) -> String {
    let time = UNIX_EPOCH + Duration::from_millis(milliseconds.max(0) as u64);
    let stamp = sigv4::amz_date(time);
    format!(
        "{}-{}-{} {}:{}",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(human_bytes(Some(0)), "0 B");
        assert_eq!(human_bytes(Some(1024)), "1.0 KiB");
        assert_eq!(human_bytes(None), "");
    }

    #[test]
    fn formats_milliseconds() {
        assert_eq!(format_epoch_ms(0), "1970-01-01 00:00");
    }

    #[test]
    fn rejects_empty_bucket_names() {
        assert!(build_create(&ViewId::S3Buckets, json!({"name": "  "})).is_err());
        assert!(build_create(&ViewId::S3Buckets, json!({"name": "ok"})).is_ok());
    }
}
