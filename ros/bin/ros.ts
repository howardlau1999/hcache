#!/usr/bin/env node
import * as ros from '@alicloud/ros-cdk-core';
import { BenchmarkStack } from '../lib/benchmark-stack';
import { DPDKStack } from '../lib/dpdk-stack';
import { TestStack } from '../lib/test-stack';

const app = new ros.App({outdir: './cdk.out'});
new TestStack(app, 'TestStack');
new BenchmarkStack(app, 'BenchmarkStack');
new DPDKStack(app, 'DPDKStack');
app.synth();