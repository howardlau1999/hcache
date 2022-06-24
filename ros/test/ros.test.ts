import { expect as expectCDK, matchTemplate, MatchStyle } from '@alicloud/ros-cdk-assert';
import * as ros from '@alicloud/ros-cdk-core';
import * as Ros from '../lib/ros-stack';

test('Stack with version.', () => {
  const app = new ros.App();
  // WHEN
  const stack = new Ros.RosStack(app, 'Tianchi Distributed Cache Service');
  // THEN
  expectCDK(stack).to(
    matchTemplate(
      {
        ROSTemplateFormatVersion: '2015-09-01',
        Description: "This is the simple ros cdk app example.",
        Metadata: {
            "ALIYUN::ROS::Interface": {
                "TemplateTags" : [
                    "Create by ROS CDK"
                ]
            }
        }
      },
      MatchStyle.EXACT,
    ),
  );
});
