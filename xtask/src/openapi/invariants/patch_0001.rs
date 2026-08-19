use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    OPERATION_RULE,
    context::{OperationView, Validator, resolve_local},
    schema::{
        component_schema, composition_refs, effective_properties, effective_shape, is_uuid_string,
        request_json_schema, response_json_schema,
    },
};

const STANDARD_RESPONSE_STATUSES: &[&str] = &["200", "400", "401", "403", "404", "405", "422"];

const AUTH_RULE: &str = "WA-AUTH-001";
const ARTICLE_RESPONSES_RULE: &str = "WA-CONTRACT-ARTICLE-RESPONSES";
const NOTESECTION_RESPONSES_RULE: &str = "WA-CONTRACT-NOTESECTION-RESPONSES";
const CANVAS_CREATE_RULE: &str = "WA-CONTRACT-CANVAS-CREATE";
const MARKER_READ_RULE: &str = "WA-CONTRACT-MARKER-READ";
const MARKER_TYPES_LIST_RULE: &str = "WA-CONTRACT-MARKER-TYPES-LIST";
const USER_UPDATE_RULE: &str = "WA-CONTRACT-USER-UPDATE";
const SCHEMAS_RULE: &str = "WA-CONTRACT-SCHEMAS";

pub(super) fn validate(validator: &mut Validator<'_>) {
    validator.validate_authentication();
    validator.validate_operation_fundamentals();
    validator.validate_contracts();
}

impl<'a> Validator<'a> {
    fn validate_authentication(&mut self) {
        let schemes = self
            .root
            .pointer("/components/securitySchemes")
            .and_then(Value::as_object);

        for (scheme_name, header_name) in [
            ("ApplicationKey", "x-application-key"),
            ("UserAuthenticationToken", "x-auth-token"),
        ] {
            let Some(scheme) = schemes.and_then(|schemes| schemes.get(scheme_name)) else {
                self.push(
                    AUTH_RULE,
                    format!("components.securitySchemes.{scheme_name}"),
                    "required security scheme is missing",
                );
                continue;
            };
            let scheme = match resolve_local(self.root, scheme) {
                Ok(scheme) => scheme,
                Err(error) => {
                    self.push(
                        AUTH_RULE,
                        format!("components.securitySchemes.{scheme_name}"),
                        error,
                    );
                    continue;
                }
            };
            if scheme.get("type").and_then(Value::as_str) != Some("apiKey")
                || scheme.get("in").and_then(Value::as_str) != Some("header")
                || scheme.get("name").and_then(Value::as_str) != Some(header_name)
            {
                self.push(
                    AUTH_RULE,
                    format!("components.securitySchemes.{scheme_name}"),
                    format!("expected apiKey header {header_name}"),
                );
            }
        }

        let Some(global_security) = self.root.get("security") else {
            self.push(
                AUTH_RULE,
                "global security",
                "missing the combined authentication requirement",
            );
            return;
        };
        if !is_combined_authentication(global_security) {
            self.push(
                AUTH_RULE,
                "global security",
                "ApplicationKey and UserAuthenticationToken must be in one AND requirement",
            );
        }

        for operation in self.operations.clone() {
            if let Some(security) = operation.operation.get("security")
                && !is_combined_authentication(security)
            {
                self.push(
                    AUTH_RULE,
                    operation.location(),
                    "operation security weakens or replaces the required two-header contract",
                );
            }
        }
    }

    fn validate_operation_fundamentals(&mut self) {
        let mut operation_ids: BTreeMap<&str, Vec<String>> = BTreeMap::new();

        for operation in self.operations.clone() {
            let Some(operation_id) = operation
                .operation
                .get("operationId")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
            else {
                self.push(
                    OPERATION_RULE,
                    operation.location(),
                    "operationId must be nonempty",
                );
                continue;
            };
            operation_ids
                .entry(operation_id)
                .or_default()
                .push(operation.location());

            let responses = operation
                .operation
                .get("responses")
                .and_then(Value::as_object);
            match responses {
                Some(responses)
                    if !responses.is_empty()
                        && responses.keys().any(|status| is_success_status(status)) => {}
                Some(_) => self.push(
                    OPERATION_RULE,
                    operation.location(),
                    "operation must declare at least one 2xx response",
                ),
                None => self.push(
                    OPERATION_RULE,
                    operation.location(),
                    "responses must be an object",
                ),
            }
        }

        for (operation_id, locations) in operation_ids {
            if locations.len() > 1 {
                self.push(
                    OPERATION_RULE,
                    format!("operationId {operation_id}"),
                    format!("operationId is duplicated at {}", locations.join(", ")),
                );
            }
        }
    }

    fn validate_contracts(&mut self) {
        for operation_id in [
            "readArticle",
            "createArticle",
            "deleteArticle",
            "updateArticle",
        ] {
            self.require_response_statuses(
                operation_id,
                STANDARD_RESPONSE_STATUSES,
                ARTICLE_RESPONSES_RULE,
            );
        }
        self.require_response_statuses(
            "listNotesectionsByNotebook",
            STANDARD_RESPONSE_STATUSES,
            NOTESECTION_RESPONSES_RULE,
        );
        self.validate_create_canvas_contract();
        self.validate_read_marker_contract();
        self.validate_list_marker_types_contract();
        self.validate_update_user_contract();
        self.validate_schema_contracts();
    }

    fn operation_by_id(&self, operation_id: &str) -> Option<OperationView<'a>> {
        self.operations
            .iter()
            .copied()
            .find(|operation| operation.operation_id() == operation_id)
    }

    fn require_response_statuses(
        &mut self,
        operation_id: &str,
        expected: &[&str],
        rule: &'static str,
    ) {
        let Some(operation) = self.operation_by_id(operation_id) else {
            self.push(
                rule,
                format!("operationId {operation_id}"),
                "required operation is missing",
            );
            return;
        };
        let Some(responses) = operation
            .operation
            .get("responses")
            .and_then(Value::as_object)
        else {
            self.push(rule, operation.location(), "responses must be an object");
            return;
        };
        let missing = expected
            .iter()
            .copied()
            .filter(|status| !responses.contains_key(*status))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            self.push(
                rule,
                operation.location(),
                format!("missing response statuses {}", missing.join(", ")),
            );
        }
    }

    fn validate_create_canvas_contract(&mut self) {
        let Some(operation) = self.operation_by_id("createCanvas") else {
            self.push(
                CANVAS_CREATE_RULE,
                "operationId createCanvas",
                "required operation is missing",
            );
            return;
        };
        let schema = request_json_schema(self.root, operation.operation);
        let Ok(schema) = schema else {
            self.push(
                CANVAS_CREATE_RULE,
                operation.location(),
                schema.unwrap_err(),
            );
            return;
        };
        let has_base =
            composition_refs(schema, "allOf").contains("#/components/schemas/canvas.two");
        let required = effective_shape(self.root, schema)
            .map(|(_, required)| required)
            .unwrap_or_default();
        if !has_base
            || !["title", "world"]
                .iter()
                .all(|field| required.contains(*field))
        {
            self.push(
                CANVAS_CREATE_RULE,
                operation.location(),
                "request schema must compose canvas.two and require title and world",
            );
        }
    }

    fn validate_read_marker_contract(&mut self) {
        let Some(operation) = self.operation_by_id("readMarker") else {
            self.push(
                MARKER_READ_RULE,
                "operationId readMarker",
                "required operation is missing",
            );
            return;
        };
        let schema = response_json_schema(self.root, operation.operation, "200");
        let Ok(schema) = schema else {
            self.push(MARKER_READ_RULE, operation.location(), schema.unwrap_err());
            return;
        };
        let refs = composition_refs(schema, "oneOf");
        let expected = [
            "#/components/schemas/marker.ref",
            "#/components/schemas/marker.zero",
            "#/components/schemas/marker.one",
            "#/components/schemas/marker.two",
        ];
        if !expected.iter().all(|reference| refs.contains(*reference)) {
            self.push(
                MARKER_READ_RULE,
                operation.location(),
                "200 JSON schema must contain every marker granularity variant",
            );
        }
    }

    fn validate_list_marker_types_contract(&mut self) {
        let Some(operation) = self.operation_by_id("listMarkerTypes") else {
            self.push(
                MARKER_TYPES_LIST_RULE,
                "operationId listMarkerTypes",
                "required operation is missing",
            );
            return;
        };
        match request_json_schema(self.root, operation.operation) {
            Ok(schema)
                if schema.get("$ref").and_then(Value::as_str)
                    == Some("#/components/schemas/General") => {}
            Ok(_) => self.push(
                MARKER_TYPES_LIST_RULE,
                operation.location(),
                "request JSON schema must reference General",
            ),
            Err(error) => self.push(MARKER_TYPES_LIST_RULE, operation.location(), error),
        }
    }

    fn validate_update_user_contract(&mut self) {
        let Some(operation) = self.operation_by_id("updateUser") else {
            self.push(
                USER_UPDATE_RULE,
                "operationId updateUser",
                "required operation is missing",
            );
            return;
        };
        match response_json_schema(self.root, operation.operation, "200") {
            Ok(schema)
                if schema.get("$ref").and_then(Value::as_str)
                    == Some("#/components/schemas/user.ref") => {}
            Ok(_) => self.push(
                USER_UPDATE_RULE,
                operation.location(),
                "200 JSON schema must reference user.ref",
            ),
            Err(error) => self.push(USER_UPDATE_RULE, operation.location(), error),
        }
    }

    fn validate_schema_contracts(&mut self) {
        self.validate_article_schema_contracts();
        self.validate_block_schema_contracts();
        self.validate_canvas_schema_contracts();
        self.validate_reference_schema_contracts();
        self.validate_read_only_contracts();
    }

    fn validate_article_schema_contracts(&mut self) {
        let Some(core) = component_schema(self.root, "ArticleGenericCore") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ArticleGenericCore",
                "required schema is missing",
            );
            return;
        };
        for name in ["userMetadata", "articleMetadata"] {
            let property = core.pointer(&format!("/properties/{name}"));
            if !property.is_some_and(|property| {
                property.get("type").and_then(Value::as_str) == Some("object")
                    && property.get("nullable").and_then(Value::as_bool) == Some(true)
                    && property.get("readOnly").and_then(Value::as_bool) == Some(true)
            }) {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.ArticleGenericCore.{name}"),
                    "expected a nullable, read-only object",
                );
            }
        }

        let Some(properties) = component_schema(self.root, "ArticleGenericObjectProperties") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ArticleGenericObjectProperties",
                "required schema is missing",
            );
            return;
        };
        let cover = properties.pointer("/properties/cover");
        if !cover.is_some_and(|cover| {
            composition_refs(cover, "allOf").contains("#/components/schemas/ImageRef")
                && cover
                    .get("allOf")
                    .and_then(Value::as_array)
                    .is_some_and(|branches| {
                        branches.iter().any(|branch| {
                            branch.get("type").and_then(Value::as_str) == Some("object")
                                && branch.get("nullable").and_then(Value::as_bool) == Some(true)
                        })
                    })
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ArticleGenericObjectProperties.cover",
                "cover must combine ImageRef with a nullable object schema",
            );
        }

        let Some(full_plus) = component_schema(self.root, "ArticleGenericFullPlus") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ArticleGenericFullPlus",
                "required schema is missing",
            );
            return;
        };
        let ancestry = effective_properties(self.root, full_plus, "ancestry").unwrap_or_default();
        if ancestry.is_empty() {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ArticleGenericFullPlus.ancestry",
                "required property is missing",
            );
            return;
        }
        let expected = [
            "#/components/schemas/ArticleRef",
            "#/components/schemas/CategoryRef",
            "#/components/schemas/world.ref",
        ];
        for ancestry in ancestry {
            for name in ["secondUp", "thirdUp"] {
                let property = ancestry.pointer(&format!("/properties/{name}"));
                let valid = property.is_some_and(|property| {
                    let refs = composition_refs(property, "oneOf");
                    property.get("type").and_then(Value::as_str) == Some("object")
                        && property.get("nullable").and_then(Value::as_bool) == Some(true)
                        && expected.iter().all(|reference| refs.contains(*reference))
                });
                if !valid {
                    self.push(
                        SCHEMAS_RULE,
                        format!("components.schemas.ArticleGenericFullPlus.ancestry.{name}"),
                        "expected a nullable object with Article, Category, and World alternatives",
                    );
                }
            }
        }
    }

    fn validate_block_schema_contracts(&mut self) {
        let Some(block) = component_schema(self.root, "Block") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.Block",
                "required schema is missing",
            );
            return;
        };
        let data_parser = effective_properties(self.root, block, "dataParser").unwrap_or_default();
        if data_parser.is_empty()
            || data_parser.iter().any(|schema| {
                schema.get("type").and_then(Value::as_str) != Some("string")
                    || schema.get("format").and_then(Value::as_str) != Some("yaml")
            })
        {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.Block.dataParser",
                "expected string/yaml",
            );
        }

        for (name, base) in [
            ("BlockTemplateExtended", "BlockTemplate"),
            ("BlockTemplateFull", "BlockTemplateExtended"),
            ("BlockTemplateUpdate", "BlockTemplate"),
            ("BlockTemplateCreate", "BlockTemplate"),
        ] {
            let Some(schema) = component_schema(self.root, name) else {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{name}"),
                    "required schema is missing",
                );
                continue;
            };
            if !composition_refs(schema, "allOf")
                .contains(format!("#/components/schemas/{base}").as_str())
            {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{name}"),
                    format!("must compose {base}"),
                );
            }
        }
        let Some(create) = component_schema(self.root, "BlockTemplateCreate") else {
            return;
        };
        let required = effective_shape(self.root, create)
            .map(|(_, required)| required)
            .unwrap_or_default();
        for field in ["title", "formSchemaParser", "formSchema", "RPGSRD"] {
            if !required.contains(field) {
                self.push(
                    SCHEMAS_RULE,
                    "components.schemas.BlockTemplateCreate",
                    format!("must require {field}"),
                );
            }
        }
    }

    fn validate_canvas_schema_contracts(&mut self) {
        const SHAPES: &[&str] = &[
            "canvas.shape.type.draw",
            "canvas.shape.type.arrow",
            "canvas.shape.type.sticky",
            "canvas.shape.type.text",
            "canvas.shape.type.block",
            "canvas.shape.type.rectangle",
        ];

        let Some(pages) = component_schema(self.root, "canvas.pages") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.canvas.pages",
                "required schema is missing",
            );
            return;
        };
        let shapes = pages.pointer("/properties/page/properties/shapes/additionalProperties");
        let valid = shapes.is_some_and(|shapes| {
            let refs = composition_refs(shapes, "oneOf");
            SHAPES
                .iter()
                .all(|name| refs.contains(format!("#/components/schemas/{name}").as_str()))
        });
        if !valid {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.canvas.pages.page.shapes",
                "additionalProperties must include all six supported shape schemas",
            );
        }

        for name in SHAPES {
            if !component_schema(self.root, name)
                .is_some_and(|schema| schema.get("type").and_then(Value::as_str) == Some("object"))
            {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{name}"),
                    "canvas shape schema must resolve to an object",
                );
            }
        }

        let Some(draw) = component_schema(self.root, "canvas.shape.type.draw") else {
            return;
        };
        let points = draw.pointer("/properties/points");
        if !points.is_some_and(|points| {
            points.get("type").and_then(Value::as_str) == Some("array")
                && points.pointer("/items/type").and_then(Value::as_str) == Some("array")
                && points.pointer("/items/items/type").and_then(Value::as_str) == Some("number")
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.canvas.shape.type.draw.points",
                "expected an array of arrays of numbers",
            );
        }
        let Some(text) = component_schema(self.root, "canvas.shape.type.text") else {
            return;
        };
        let point = text.pointer("/properties/point");
        if !point.is_some_and(|point| {
            point.get("type").and_then(Value::as_str) == Some("array")
                && point.pointer("/items/type").and_then(Value::as_str) == Some("number")
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.canvas.shape.type.text.point",
                "expected an array of numbers",
            );
        }

        let Some(arrow) = component_schema(self.root, "canvas.shape.type.arrow") else {
            return;
        };
        let start_point = arrow.pointer("/properties/handles/properties/start/properties/point");
        if !start_point.is_some_and(|point| {
            point.get("type").and_then(Value::as_str) == Some("array")
                && point.pointer("/items/type").and_then(Value::as_str) == Some("number")
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.canvas.shape.type.arrow.handles.start.point",
                "expected an array of numbers",
            );
        }
    }

    fn validate_reference_schema_contracts(&mut self) {
        let Some(manuscript) = component_schema(self.root, "ManuscriptRef") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ManuscriptRef",
                "required schema is missing",
            );
            return;
        };
        let tags = manuscript.pointer("/properties/tags");
        if !tags.is_some_and(|tags| {
            tags.get("type").and_then(Value::as_str) == Some("object")
                && tags.get("additionalProperties").and_then(Value::as_bool) == Some(true)
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.ManuscriptRef.tags",
                "expected an open object map",
            );
        }

        for (component, property) in [
            ("markergroup.update", "subscribergroups"),
            ("subscribergroup.update", "paidsubscribers"),
            ("timeline.create", "subscribergroups"),
            ("timeline.create", "histories"),
            ("timeline.update", "subscribergroups"),
            ("timeline.update", "histories"),
        ] {
            let Some(schema) = component_schema(self.root, component) else {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{component}"),
                    "required schema is missing",
                );
                continue;
            };
            let id = schema.pointer(&format!("/properties/{property}/items/properties/id"));
            if !is_uuid_string(id) {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{component}.{property}.items.id"),
                    "expected string/uuid",
                );
            }
        }

        let Some(history) = component_schema(self.root, "history.category.schema") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.history.category.schema",
                "required schema is missing",
            );
            return;
        };
        let history_properties = history.get("properties").and_then(Value::as_object);
        let expected_history_properties = [
            "id",
            "title",
            "slug",
            "state",
            "isWip",
            "isDraft",
            "entityClass",
            "icon",
            "url",
            "subscribergroups",
            "folderId",
            "tags",
            "updateDate",
        ];
        if !is_uuid_string(history.pointer("/properties/id"))
            || !history_properties.is_some_and(|properties| {
                expected_history_properties
                    .iter()
                    .all(|name| properties.contains_key(*name))
            })
        {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.history.category.schema",
                "corrected properties must remain direct, including id: string/uuid",
            );
        }

        let Some(user) = component_schema(self.root, "user.two") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.user.two",
                "required schema is missing",
            );
            return;
        };
        let onboarding = user.pointer("/properties/onboardingProgress");
        if !onboarding.is_some_and(|schema| {
            schema.get("type").and_then(Value::as_str) == Some("integer")
                && schema.get("format").and_then(Value::as_str) == Some("int32")
        }) {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.user.two.onboardingProgress",
                "expected integer/int32",
            );
        }

        let Some(create) = component_schema(self.root, "VariableCollectionCreate") else {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.VariableCollectionCreate",
                "required schema is missing",
            );
            return;
        };
        let required = effective_shape(self.root, create)
            .map(|(_, required)| required)
            .unwrap_or_default();
        if !composition_refs(create, "allOf")
            .contains("#/components/schemas/VariableCollectionUpdate")
            || !["title", "world"]
                .iter()
                .all(|field| required.contains(*field))
        {
            self.push(
                SCHEMAS_RULE,
                "components.schemas.VariableCollectionCreate",
                "must compose VariableCollectionUpdate and require title and world",
            );
        }
    }

    fn validate_read_only_contracts(&mut self) {
        for (component, root_read_only, properties) in [
            ("customarticletemplate.ref", false, &["id", "title"][..]),
            (
                "OrgChartRef",
                true,
                &[
                    "id",
                    "title",
                    "slug",
                    "state",
                    "isWip",
                    "isDraft",
                    "entityClass",
                    "icon",
                    "url",
                    "subscribergroups",
                    "folderId",
                    "tags",
                ][..],
            ),
            (
                "PromptRef",
                true,
                &[
                    "id",
                    "title",
                    "slug",
                    "state",
                    "isWip",
                    "isDraft",
                    "entityClass",
                    "icon",
                    "url",
                    "subscribergroups",
                    "folderId",
                    "tags",
                ][..],
            ),
        ] {
            let Some(schema) = component_schema(self.root, component) else {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{component}"),
                    "required schema is missing",
                );
                continue;
            };
            if root_read_only && schema.get("readOnly").and_then(Value::as_bool) != Some(true) {
                self.push(
                    SCHEMAS_RULE,
                    format!("components.schemas.{component}.readOnly"),
                    "expected readOnly: true",
                );
            }
            for property in properties {
                if schema
                    .pointer(&format!("/properties/{property}/readOnly"))
                    .and_then(Value::as_bool)
                    != Some(true)
                {
                    self.push(
                        SCHEMAS_RULE,
                        format!("components.schemas.{component}.{property}.readOnly"),
                        "expected readOnly: true",
                    );
                }
            }
        }
    }
}

fn is_combined_authentication(security: &Value) -> bool {
    let Some(requirements) = security.as_array() else {
        return false;
    };
    if requirements.len() != 1 {
        return false;
    }
    let Some(requirement) = requirements[0].as_object() else {
        return false;
    };
    requirement.len() == 2
        && ["ApplicationKey", "UserAuthenticationToken"]
            .iter()
            .all(|name| {
                requirement
                    .get(*name)
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
            })
}

fn is_success_status(status: &str) -> bool {
    status.eq_ignore_ascii_case("2XX")
        || status
            .parse::<u16>()
            .is_ok_and(|status| (200..=299).contains(&status))
}
