#!/usr/bin/env node
import * as ros from '@alicloud/ros-cdk-core';
import { SubmissionStack } from '../lib/submission-stack';
import { TestStack } from '../lib/test-stack';

const app = new ros.App({outdir: './cdk.out'});
new SubmissionStack(app, 'SubmissionStack');
new TestStack(app, 'TestStack');
app.synth();