//! The identity used when the native application publishes a response.
use mant_engine::QueryViewResult;
use mant_protocol::{Producer, ScopeQueryResponse, ScopeQueryResult};

pub(crate) const NAME: &str = env!("CARGO_PKG_NAME");
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn process_producer() -> Producer {
    Producer {
        name: NAME.to_owned(),
        version: VERSION.to_owned(),
        engine: None,
    }
}

pub(crate) fn identify(producer: &mut Producer) {
    NAME.clone_into(&mut producer.name);
    VERSION.clone_into(&mut producer.version);
}

pub(crate) fn identify_query_projection(result: &mut QueryViewResult) {
    let producer = match result {
        QueryViewResult::Excerpt(excerpt) => excerpt.producer.as_mut(),
        QueryViewResult::Explanation(explanation) => explanation.producer.as_mut(),
        QueryViewResult::Full(_) | QueryViewResult::Outline(_) | QueryViewResult::Search(_) => None,
    };
    if let Some(producer) = producer {
        identify(producer);
    }
}

pub(crate) fn identify_scope_query(response: &mut ScopeQueryResponse) {
    if let ScopeQueryResult::Explain { explanation } = &mut response.result {
        for producer in explanation
            .documents
            .iter_mut()
            .filter_map(|document| document.producer.as_mut())
        {
            identify(producer);
        }
    }
}
