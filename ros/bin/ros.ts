#!/usr/bin/env node
import * as ros from '@alicloud/ros-cdk-core';
import { RosStack } from '../lib/ros-stack';

const app = new ros.App({outdir: './cdk.out'});
new RosStack(app, 'RosStack');
app.synth();