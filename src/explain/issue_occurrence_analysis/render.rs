//! Bounded human projection for validated issue occurrence analysis.

use serde_json::Value;

use super::super::{
    append_labeled_basis_points, append_labeled_integer, append_labeled_text, field_text,
};

/// Appends all validated fixed buckets and top values for human or agent inspection.
pub(super) fn render(output: &mut String, value: Option<&Value>) {
    let Some(analysis) = value else {
        return;
    };
    output.push_str("Occurrence analysis:");
    append_labeled_text(output, "status", analysis, "status", 32);
    if let Some(coverage) = analysis.get("coverage") {
        append_labeled_integer(output, "retained", coverage, "retained_occurrences");
        append_labeled_integer(output, "trend", coverage, "trend_occurrences");
        append_labeled_integer(
            output,
            "distributions",
            coverage,
            "available_distribution_count",
        );
        output.push('/');
        output.push_str(
            coverage
                .get("expected_distribution_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .to_string()
                .as_str(),
        );
    }
    output.push('\n');
    render_trend(output, analysis.get("trend"));
    if let Some(distributions) = analysis.get("distributions").and_then(Value::as_array) {
        for distribution in distributions {
            render_distribution(output, distribution);
        }
    }
    if let Some(limitations) = analysis.get("limitations").and_then(Value::as_array)
        && !limitations.is_empty()
    {
        output.push_str("Occurrence-analysis limitations:");
        for limitation in limitations.iter().filter_map(Value::as_str) {
            output.push(' ');
            output.push_str(limitation);
        }
        output.push('\n');
    }
}

/// Appends one bounded zero-filled trend.
fn render_trend(output: &mut String, value: Option<&Value>) {
    let Some(trend) = value.filter(|value| !value.is_null()) else {
        output.push_str("Occurrence trend: unavailable.\n");
        return;
    };
    output.push_str("Occurrence trend:");
    if let (Some(start), Some(end)) = (
        field_text(trend, "scope_start", 64),
        field_text(trend, "scope_end", 64),
    ) {
        output.push_str(" scope=");
        output.push_str(start.as_str());
        output.push_str("..");
        output.push_str(end.as_str());
    }
    append_labeled_integer(output, "interval", trend, "interval_seconds");
    output.push('s');
    let buckets = trend.get("buckets").and_then(Value::as_array);
    output.push_str(" buckets=");
    output.push_str(buckets.map_or(0, Vec::len).to_string().as_str());
    output.push('\n');
    if let Some(buckets) = buckets {
        for bucket in buckets {
            output.push_str("Trend bucket:");
            append_labeled_text(output, "start", bucket, "bucket_start", 64);
            append_labeled_text(output, "end", bucket, "bucket_end", 64);
            append_labeled_integer(output, "occurrences", bucket, "occurrence_count");
            output.push('\n');
        }
    }
}

/// Appends one bounded exact distribution and every named value.
fn render_distribution(output: &mut String, value: &Value) {
    output.push_str("Occurrence distribution:");
    append_labeled_text(output, "dimension", value, "dimension", 32);
    append_labeled_integer(output, "distinct", value, "distinct_value_count");
    output.push_str(" shown=");
    output.push_str(
        value
            .get("values")
            .and_then(Value::as_array)
            .map_or(0, Vec::len)
            .to_string()
            .as_str(),
    );
    append_labeled_integer(output, "other", value, "other_occurrence_count");
    output.push('\n');
    if let Some(values) = value.get("values").and_then(Value::as_array) {
        for value in values {
            output.push_str("Distribution value:");
            append_labeled_text(output, "value", value, "value", 240);
            append_labeled_text(output, "version", value, "version", 120);
            append_labeled_integer(output, "occurrences", value, "occurrence_count");
            append_labeled_basis_points(output, "share", value, "share_basis_points");
            output.push('\n');
        }
    }
}
