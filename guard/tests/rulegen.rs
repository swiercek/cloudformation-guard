// Copyright Amazon Web Services, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod utils;

#[cfg(test)]
mod rulegen_tests {
    use std::io::stdout;

    use rstest::rstest;

    use crate::assert_output_from_file_eq;
    use cfn_guard::commands::{OUTPUT, RULEGEN, TEMPLATE};
    use cfn_guard::utils::writer::WriteBuffer::Stderr;
    use cfn_guard::utils::writer::{WriteBuffer::Stdout, WriteBuffer::Vec as WBVec, Writer};
    use cfn_guard::Error;

    use crate::utils::{get_full_path_for_resource_file, CommandTestRunner, StatusCode};

    #[derive(Default)]
    struct RulegenTestRunner<'args> {
        template: Option<&'args str>,
        output: Option<&'args str>,
    }

    impl<'args> RulegenTestRunner<'args> {
        fn template(&'args mut self, arg: Option<&'args str>) -> &'args mut RulegenTestRunner {
            self.template = arg;
            self
        }

        fn output(&'args mut self, arg: Option<&'args str>) -> &'args mut RulegenTestRunner {
            self.output = arg;
            self
        }
    }

    impl<'args> CommandTestRunner for RulegenTestRunner<'args> {
        fn build_args(&self) -> Vec<String> {
            let mut args = vec![String::from(RULEGEN)];

            if self.template.is_some() {
                args.push(format!("-{}", TEMPLATE.1));
                args.push(get_full_path_for_resource_file(self.template.unwrap()));
            }

            if self.output.is_some() {
                args.push(format!("-{}", OUTPUT.1));
                args.push(get_full_path_for_resource_file(self.output.unwrap()))
            }

            args
        }
    }

    #[rstest::rstest]
    #[case(
        Some("resources/rulegen/data-dir/s3-public-read-prohibited-template-compliant.json"),
        "resources/rulegen/output-dir/test_rulegen_from_template.out",
        StatusCode::SUCCESS
    )]
    #[case(
        Some("resources/rulegen/data-dir/s3-public-read-prohibited-template-compliant.yaml"),
        "resources/rulegen/output-dir/test_rulegen_from_template.out",
        StatusCode::SUCCESS
    )]
    fn test_rulegen_from_template(
        #[case] template_arg: Option<&str>,
        #[case] expected_output_file_path: &str,
        #[case] expected_status_code: i32,
    ) {
        let mut writer = Writer::new(WBVec(vec![]), Stderr(std::io::stderr()));
        let status_code = RulegenTestRunner::default()
            .template(template_arg)
            .run(&mut writer);

        assert_eq!(expected_status_code, status_code);
        assert_output_from_file_eq!(expected_output_file_path, writer)
    }
}
