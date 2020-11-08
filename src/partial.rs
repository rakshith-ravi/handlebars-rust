use std::collections::HashMap;

use serde_json::value::Value as Json;

use crate::context::{merge_json, Context};
use crate::error::RenderError;
use crate::json::path::Path;
use crate::output::Output;
use crate::registry::Registry;
use crate::render::{Decorator, Evaluable, RenderContext, Renderable};
use crate::template::Template;

const PARTIAL_BLOCK: &str = "@partial-block";

fn render_partial<'reg: 'rc, 'rc: 'blk, 'blk>(
    t: &'reg Template,
    d: &Decorator<'reg, 'rc>,
    r: &'reg Registry<'reg>,
    local_rc: &'rc mut RenderContext<'reg, 'rc, 'blk>,
) -> Result<(), RenderError> {
    // partial context path
    if let Some(ref param_ctx) = d.param(0) {
        if let (Some(p), Some(block)) = (param_ctx.context_path(), local_rc.block_mut()) {
            *block.base_path_mut() = p.clone();
        }
    }

    // @partial-block
    if let Some(t) = d.template() {
        local_rc.set_partial(PARTIAL_BLOCK.to_owned(), t);
    }

    let result = if d.hash().is_empty() {
        t.render(r, local_rc)
    } else {
        let hash_ctx = d
            .hash()
            .iter()
            .map(|(k, v)| (k, v.value()))
            .collect::<HashMap<&&str, &Json>>();
        let current_path = Path::current();
        let partial_context = merge_json(local_rc.evaluate2(&current_path)?.as_json(), &hash_ctx);

        // FIXME
        let ctx = Context::wraps(&partial_context)?;
        let mut partial_rc = local_rc.new_for_block();

        t.render(r, &mut partial_rc)
    };

    local_rc.remove_partial(PARTIAL_BLOCK);

    result
}

pub fn expand_partial<'reg: 'rc, 'rc: 'blk, 'blk>(
    d: &Decorator<'reg, 'rc>,
    r: &'reg Registry<'reg>,
    rc: &'rc mut RenderContext<'reg, 'rc, 'blk>,
) -> Result<(), RenderError> {
    // try eval inline partials first
    if let Some(t) = d.template() {
        t.eval(r, rc)?;
    }

    let tname = d.name();
    if rc.is_current_template(tname) {
        return Err(RenderError::new("Cannot include self in >"));
    }

    let partial = rc.get_partial(tname);

    match partial {
        Some(t) => {
            let mut local_rc = rc.new_for_block();
            render_partial(&t, d, r, &mut local_rc)?;
        }
        None => {
            if let Some(t) = r.get_template(tname).or_else(|| d.template()) {
                let mut local_rc = rc.new_for_block();
                render_partial(t, d, r, &mut local_rc)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::context::Context;
    use crate::error::RenderError;
    use crate::output::Output;
    use crate::registry::Registry;
    use crate::render::{Helper, RenderContext};

    #[test]
    fn test() {
        let mut handlebars = Registry::new();
        assert!(handlebars
            .register_template_string("t0", "{{> t1}}")
            .is_ok());
        assert!(handlebars
            .register_template_string("t1", "{{this}}")
            .is_ok());
        assert!(handlebars
            .register_template_string("t2", "{{#> t99}}not there{{/t99}}")
            .is_ok());
        assert!(handlebars
            .register_template_string("t3", "{{#*inline \"t31\"}}{{this}}{{/inline}}{{> t31}}")
            .is_ok());
        assert!(handlebars
            .register_template_string(
                "t4",
                "{{#> t5}}{{#*inline \"nav\"}}navbar{{/inline}}{{/t5}}"
            )
            .is_ok());
        assert!(handlebars
            .register_template_string("t5", "include {{> nav}}")
            .is_ok());
        assert!(handlebars
            .register_template_string("t6", "{{> t1 a}}")
            .is_ok());
        assert!(handlebars
            .register_template_string(
                "t7",
                "{{#*inline \"t71\"}}{{a}}{{/inline}}{{> t71 a=\"world\"}}"
            )
            .is_ok());
        assert!(handlebars.register_template_string("t8", "{{a}}").is_ok());
        assert!(handlebars
            .register_template_string("t9", "{{> t8 a=2}}")
            .is_ok());

        assert_eq!(handlebars.render("t0", &1).ok().unwrap(), "1".to_string());
        assert_eq!(
            handlebars.render("t2", &1).ok().unwrap(),
            "not there".to_string()
        );
        assert_eq!(handlebars.render("t3", &1).ok().unwrap(), "1".to_string());
        assert_eq!(
            handlebars.render("t4", &1).ok().unwrap(),
            "include navbar".to_string()
        );
        assert_eq!(
            handlebars
                .render("t6", &btreemap! {"a".to_string() => "2".to_string()})
                .ok()
                .unwrap(),
            "2".to_string()
        );
        assert_eq!(
            handlebars.render("t7", &1).ok().unwrap(),
            "world".to_string()
        );
        assert_eq!(handlebars.render("t9", &1).ok().unwrap(), "2".to_string());
    }

    #[test]
    fn test_include_partial_block() {
        let t0 = "hello {{> @partial-block}}";
        let t1 = "{{#> t0}}inner {{this}}{{/t0}}";

        let mut handlebars = Registry::new();
        assert!(handlebars.register_template_string("t0", t0).is_ok());
        assert!(handlebars.register_template_string("t1", t1).is_ok());

        let r0 = handlebars.render("t1", &true);
        assert_eq!(r0.ok().unwrap(), "hello inner true".to_string());
    }

    #[test]
    fn test_self_inclusion() {
        let t0 = "hello {{> t1}} {{> t0}}";
        let t1 = "some template";
        let mut handlebars = Registry::new();
        assert!(handlebars.register_template_string("t0", t0).is_ok());
        assert!(handlebars.register_template_string("t1", t1).is_ok());

        let r0 = handlebars.render("t0", &true);
        assert!(r0.is_err());
    }

    #[test]
    fn test_issue_143() {
        let main_template = "one{{> two }}three{{> two }}";
        let two_partial = "--- two ---";

        let mut handlebars = Registry::new();
        assert!(handlebars
            .register_template_string("template", main_template)
            .is_ok());
        assert!(handlebars
            .register_template_string("two", two_partial)
            .is_ok());

        let r0 = handlebars.render("template", &true);
        assert_eq!(r0.ok().unwrap(), "one--- two ---three--- two ---");
    }

    #[test]
    fn test_hash_context_outscope() {
        let main_template = "In: {{> p a=2}} Out: {{a}}";
        let p_partial = "{{a}}";

        let mut handlebars = Registry::new();
        assert!(handlebars
            .register_template_string("template", main_template)
            .is_ok());
        assert!(handlebars.register_template_string("p", p_partial).is_ok());

        let r0 = handlebars.render("template", &true);
        assert_eq!(r0.ok().unwrap(), "In: 2 Out: ");
    }

    #[test]
    fn test_partial_context_hash() {
        let mut hbs = Registry::new();
        hbs.register_template_string("one", "This is a test. {{> two name=\"fred\" }}")
            .unwrap();
        hbs.register_template_string("two", "Lets test {{name}}")
            .unwrap();
        assert_eq!(
            "This is a test. Lets test fred",
            hbs.render("one", &0).unwrap()
        );
    }

    #[test]
    fn test_partial_subexpression_context_hash() {
        let mut hbs = Registry::new();
        hbs.register_template_string("one", "This is a test. {{> (x @root) name=\"fred\" }}")
            .unwrap();
        hbs.register_template_string("two", "Lets test {{name}}")
            .unwrap();

        hbs.register_helper(
            "x",
            Box::new(
                |_: &Helper<'_, '_>,
                 _: &Registry<'_>,
                 _: &Context,
                 _: &mut RenderContext<'_, '_>,
                 out: &mut dyn Output|
                 -> Result<(), RenderError> {
                    out.write("two")?;
                    Ok(())
                },
            ),
        );
        assert_eq!(
            "This is a test. Lets test fred",
            hbs.render("one", &0).unwrap()
        );
    }

    #[test]
    fn test_nested_partial_scope() {
        let t = "{{#*inline \"pp\"}}{{a}} {{b}}{{/inline}}{{#each c}}{{> pp a=2}}{{/each}}";
        let data = json!({"c": [{"b": true}, {"b": false}]});

        let mut handlebars = Registry::new();
        assert!(handlebars.register_template_string("t", t).is_ok());
        let r0 = handlebars.render("t", &data);
        assert_eq!(r0.ok().unwrap(), "2 true2 false");
    }
}
