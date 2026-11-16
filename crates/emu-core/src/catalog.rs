//! The service catalog.
//!
//! The catalog is generated from the botocore service models (the same models
//! the AWS CLI and SDKs use) by `scripts/generate-catalog.py`, then gzipped and
//! embedded. It contains, for every service: wire protocol, SigV4 signing name,
//! API version, and every operation with its required input skeleton and HTTP
//! binding. That is what lets the console drive services it has no bespoke code
//! for.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::OnceLock;

use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::Value;

const CATALOG_GZ: &[u8] = include_bytes!("../assets/catalog.json.gz");

/// One operation from a service model.
#[derive(Debug, Clone, Deserialize)]
pub struct OpMeta {
    /// Skeleton of the required input, with placeholder values.
    #[serde(default)]
    pub skel: Value,
    /// HTTP method for REST protocols (`rest-json`, `rest-xml`).
    pub method: Option<String>,
    /// Request URI template for REST protocols, with `{Member}` placeholders.
    pub uri: Option<String>,
    /// `(member name, location name)` pairs bound to the URI.
    #[serde(default)]
    pub labels: Vec<(String, String)>,
    /// `(member name, location name)` pairs bound to the query string.
    #[serde(rename = "queryMembers", default)]
    pub query_members: Vec<(String, String)>,
    /// `(member name, location name)` pairs bound to headers.
    #[serde(rename = "headerMembers", default)]
    pub header_members: Vec<(String, String)>,
    /// Member sent as the HTTP payload, if any.
    pub payload: Option<String>,
}

/// One service from a service model.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceMeta {
    /// Short display name, for example `Amazon S3`.
    pub display: String,
    /// Long display name.
    pub full: String,
    /// Wire protocol: `json`, `query`, `ec2`, `rest-json` or `rest-xml`.
    pub protocol: Option<String>,
    /// `X-Amz-Target` prefix for JSON protocols.
    pub target: Option<String>,
    /// API version, used by JSON targets and query requests.
    pub version: Option<String>,
    /// SigV4 signing name. This is the routing key the emulator uses.
    pub signing: String,
    /// `serviceId` from the model.
    pub service_id: Option<String>,
    /// `application/x-amz-json-<version>` version.
    pub json_version: Option<String>,
    /// Whether the JSON service is query-compatible (SQS); adds a header.
    pub query_compatible: bool,
    /// Operations by name.
    pub ops: BTreeMap<String, OpMeta>,
}

impl ServiceMeta {
    pub fn protocol(&self) -> &str {
        self.protocol.as_deref().unwrap_or("")
    }

    pub fn operation(&self, name: &str) -> Option<&OpMeta> {
        self.ops.get(name)
    }

    pub fn op_count(&self) -> usize {
        self.ops.len()
    }
}

/// The embedded catalog.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    /// Where the catalog came from (botocore version).
    pub source: String,
    /// Services keyed by endpoint prefix, for example `s3` or `dynamodb`.
    pub services: BTreeMap<String, ServiceMeta>,
}

impl Catalog {
    pub fn get(&self, key: &str) -> Option<&ServiceMeta> {
        self.services.get(key)
    }

    pub fn len(&self) -> usize {
        self.services.len()
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// Service keys ordered by category, then key.
    pub fn ordered_keys(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self.services.keys().map(String::as_str).collect();
        keys.sort_by(|a, b| category(a).cmp(category(b)).then_with(|| a.cmp(b)));
        keys
    }

    /// Keys matching `needle` in the key, display name, or signing name.
    pub fn search(&self, needle: &str) -> Vec<&str> {
        let needle = needle.to_ascii_lowercase();
        self.ordered_keys()
            .into_iter()
            .filter(|key| {
                let meta = &self.services[*key];
                key.contains(&needle)
                    || meta.display.to_ascii_lowercase().contains(&needle)
                    || meta.signing.to_ascii_lowercase().contains(&needle)
            })
            .collect()
    }
}

/// The process-wide catalog. Parsing happens once, on first use.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut decoder = GzDecoder::new(CATALOG_GZ);
        let mut json = String::new();
        decoder
            .read_to_string(&mut json)
            .expect("embedded catalog is valid gzip");
        serde_json::from_str(&json).expect("embedded catalog is valid JSON")
    })
}

/// Coarse grouping used by the service browser.
pub fn category(key: &str) -> &'static str {
    match key {
        "s3" | "s3control" | "s3tables" | "s3vectors" | "s3files" | "s3outposts" | "efs"
        | "fsx" | "backup" | "backupgateway" | "storagegateway" | "glacier" | "ebs" => "Storage",
        "dynamodb" | "dynamodbstreams" | "rds" | "rdsdata" | "docdb" | "docdbelastic"
        | "neptune" | "neptunedata" | "neptunegraph" | "elasticache" | "memorydb" | "redshift"
        | "redshiftdata" | "redshiftserverless" | "dsql" | "keyspaces" | "timestreamquery"
        | "timestreamwrite" | "timestreaminfluxdb" | "odb" | "simpledbv2" => "Databases",
        "ec2"
        | "ec2instanceconnect"
        | "lambda"
        | "lambdacore"
        | "lambdaweb"
        | "batch"
        | "autoscaling"
        | "autoscalingplans"
        | "lightsail"
        | "apprunner"
        | "imagebuilder"
        | "launchwizard"
        | "pcs"
        | "workspacesinstances"
        | "interconnect"
        | "evs" => "Compute",
        "ecs" | "eks" | "eksauth" | "ecr" | "ecrpublic" | "kafka" | "kafkaconnect" => "Containers",
        "sqs" | "sns" | "events" | "eventbridgev2" | "scheduler" | "pipes" | "mq" | "kinesis"
        | "firehose" | "kinesisanalytics" | "kinesisanalyticsv2" | "kinesisvideo" => "Messaging",
        "sfn" | "swf" | "stepfunctions" => "Orchestration",
        "iam"
        | "sts"
        | "kms"
        | "secretsmanager"
        | "acm"
        | "acmpca"
        | "waf"
        | "wafv2"
        | "wafregional"
        | "shield"
        | "guardduty"
        | "inspector"
        | "inspector2"
        | "macie2"
        | "cognitoidentity"
        | "cognitoidentityprovider"
        | "cognitosync"
        | "verifiedpermissions"
        | "securityhub"
        | "securitylake"
        | "detective"
        | "frauddetector"
        | "signer"
        | "privateca"
        | "rolesanywhere"
        | "paymentcryptography" => "Security",
        "cloudformation"
        | "cloudwatch"
        | "cloudwatchlogs"
        | "cloudtrail"
        | "cloudtraildata"
        | "ssm"
        | "ssmcontacts"
        | "ssmincidents"
        | "ssmquicksetup"
        | "ssmsap"
        | "configservice"
        | "organizations"
        | "servicecatalog"
        | "resourcegroups"
        | "resourcegroupstaggingapi"
        | "servicequotas"
        | "support"
        | "health"
        | "observabilityadmin"
        | "applicationinsights"
        | "applicationsignals"
        | "cloudcontrol"
        | "computeoptimizer"
        | "trustedadvisor"
        | "costexplorer"
        | "costoptimizationhub"
        | "budgets"
        | "billing"
        | "ce" => "Management",
        "route53"
        | "route53domains"
        | "route53resolver"
        | "route53profiles"
        | "route53recoverycontrolconfig"
        | "route53recoveryreadiness"
        | "servicediscovery"
        | "elasticloadbalancing"
        | "elasticloadbalancingv2"
        | "cloudfront"
        | "cloudfrontkeyvaluestore"
        | "apigateway"
        | "apigatewayv2"
        | "directconnect"
        | "globalaccelerator"
        | "networkmanager"
        | "networkfirewall"
        | "networkmonitor"
        | "vpclattice"
        | "transitgateway" => "Networking",
        "athena"
        | "glue"
        | "databrew"
        | "emr"
        | "emrcontainers"
        | "emrserverless"
        | "quicksight"
        | "lakeformation"
        | "opensearch"
        | "opensearchserverless"
        | "elasticsearchservice"
        | "dataexchange"
        | "datazone"
        | "datapipeline"
        | "datasync"
        | "dms"
        | "kendra"
        | "entityresolution"
        | "cleanrooms"
        | "cleanroomsml" => "Analytics",
        "bedrock"
        | "bedrockagent"
        | "bedrockagentcore"
        | "bedrockagentcorecontrol"
        | "bedrockagentruntime"
        | "bedrockdataautomation"
        | "bedrockdataautomationruntime"
        | "bedrockruntime"
        | "sagemaker"
        | "sagemakerruntime"
        | "sagemakermetrics"
        | "sagemakerfeaturestoreruntime"
        | "comprehend"
        | "comprehendmedical"
        | "textract"
        | "rekognition"
        | "polly"
        | "transcribe"
        | "transcribestreaming"
        | "translate"
        | "personalize"
        | "personalizeevents"
        | "personalizeruntime"
        | "forecast"
        | "forecastquery"
        | "lexmodelsv2"
        | "lexruntimev2"
        | "qbusiness"
        | "qapps"
        | "novaact"
        | "voiceid"
        | "elementalinference" => "AI & ML",
        "codebuild"
        | "codepipeline"
        | "codecommit"
        | "codedeploy"
        | "codeartifact"
        | "codeconnections"
        | "codestarconnections"
        | "codestarnotifications"
        | "codecatalyst"
        | "codeguruprofiler"
        | "codegurureviewer"
        | "codegurusecurity"
        | "xray"
        | "appconfig"
        | "appconfigdata"
        | "cloud9"
        | "devopsagent"
        | "devopsguru"
        | "schemas" => "Developer Tools",
        _ => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_has_core_services() {
        let catalog = catalog();
        assert!(catalog.len() > 300);
        for key in [
            "s3",
            "dynamodb",
            "sqs",
            "sns",
            "lambda",
            "iam",
            "ec2",
            "cloudformation",
            "logs",
        ] {
            let service = catalog.get(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(!service.ops.is_empty());
            assert!(!service.signing.is_empty());
        }
    }

    #[test]
    fn dynamodb_has_a_json_target() {
        let dynamodb = catalog().get("dynamodb").unwrap();
        assert_eq!(dynamodb.protocol(), "json");
        assert_eq!(dynamodb.target.as_deref(), Some("DynamoDB_20120810"));
        assert!(dynamodb.operation("ListTables").is_some());
    }
}
