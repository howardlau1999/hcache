import * as ros from '@alicloud/ros-cdk-core';
import * as ecs from '@alicloud/ros-cdk-ecs';
import * as ROS from '@alicloud/ros-cdk-ros';
import { readFileSync } from 'fs';
import { hostname } from 'os';


const imageAndStartScript = { 
  "custom": {
    imageId: process.env.HCACHE_IMAGE_ID,
    startScript: `#!/bin/bash
    NOTIFY
    `,
  }
}

export class BenchmarkStack extends ros.Stack {
  constructor(scope: ros.Construct, id: string, props?: ros.StackProps) {
    super(scope, id, props);
    new ros.RosInfo(this, ros.RosInfo.description, "部署两台 ECS，一台客户端一台服务器用来 Benchmark");
    // The code that defines your stack goes here

    // 指定使用的镜像和启动脚本
    const fromWhich = process.env.IMAGE || "custom";
    const spec = (imageAndStartScript as any)[fromWhich];
    const specImageId = spec.imageId;
    const specStartScript = spec.startScript;

    // 随机选择一个可用区部署
    const zoneId = ros.Fn.select(0, ros.Fn.getAzs(ros.RosPseudo.region));

    // 创建虚拟网络
    // 构建 VPC
    const vpc = new ecs.Vpc(this, 'hcache-bench-vpc', {
      vpcName: 'hcache-bench-vpc',
      cidrBlock: '192.168.0.0/16',
      description: 'hcache benchmark vpc'
    });

    // 构建 VSwitch
    const vswitch = new ecs.VSwitch(this, 'hcache-bench-vswitch', {
      vpcId: vpc.attrVpcId,
      zoneId: zoneId,
      vSwitchName: 'hcache-bench-vswitch',
      cidrBlock: '192.168.233.0/24',
    });

    // 指定系统镜像、系统密码、实例类型
    const ecsImageId = new ros.RosParameter(this, "ecs_image_id", {
      type: ros.RosParameterType.STRING,
      defaultValue: specImageId,
    });
    const ecsPassword = new ros.RosParameter(this, "ecs_password", {
      type: ros.RosParameterType.STRING,
      noEcho: true, minLength: 8, maxLength: 30,
      allowedPattern: "[0-9A-Za-z\\_\\-\\&:;'<>,=%`~!@#\\(\\)\\$\\^\\*\\+\\|\\{\\}\\[\\]\\.\\?\\/]+$",
      defaultValue: "hcache@2022",
    });
    const ecsInstanceType = new ros.RosParameter(this, "ecs_instance_type", {
      type: ros.RosParameterType.STRING,
      defaultValue: "ecs.c7.2xlarge",
      associationProperty: "ALIYUN::ECS::Instance::InstanceType",
      associationPropertyMetadata: {
        "ZoneId": zoneId,
      },
    });
    const ecsSystemDiskCategory = new ros.RosParameter(this, "ecs_system_disk_category", {
      type: ros.RosParameterType.STRING,
      defaultValue: "cloud_essd",
    });

    // 创建安全组开放端口
    const sg = new ecs.SecurityGroup(this, 'hcahce-sg', { vpcId: vpc.attrVpcId });

    let ports = ['22', '8080', '80', '443'];
    for (const port of ports) {
      new ecs.SecurityGroupIngress(this, `hcache-sg-ingress-${port}`, {
        portRange: `${port}/${port}`,
        nicType: 'intranet',
        sourceCidrIp: '0.0.0.0/0',
        ipProtocol: 'tcp',
        securityGroupId: sg.attrSecurityGroupId
      }, true);
    }

    // 密钥导入，默认读取本地的公钥
    const pubKey = readFileSync(`${process.env.HOME}/.ssh/id_rsa.pub`).toString();
    const keyPair = new ecs.SSHKeyPair(this, 'hcache-key-pair', {
      keyPairName: `hcache-key-pair-${hostname()}`,
      publicKeyBody: pubKey,
      tags: [{ key: 'hcache', value: hostname() }],
    });

    // 等待逻辑，用于等待 ECS 中应用安装完成
    const ecsWaitConditionHandle = new ROS.WaitConditionHandle(this, 'RosWaitConditionHandle', {
      count: 2,
    });

    const ecsWaitCondition = new ROS.WaitCondition(this, 'RosWaitCondition', {
      timeout: 600,
      handle: ros.Fn.ref('RosWaitConditionHandle'),
      count: 2
    });

    const clientInstance = new ecs.Instance(this, 'hcache-client', {
      vpcId: vpc.attrVpcId,
      keyPairName: keyPair.attrKeyPairName,
      vSwitchId: vswitch.attrVSwitchId,
      imageId: ecsImageId,
      securityGroupId: sg.attrSecurityGroupId,
      instanceType: ecsInstanceType,
      instanceName: 'hcache-benchmark-client',
      systemDiskCategory: ecsSystemDiskCategory,
      password: ecsPassword,
      spotStrategy: 'SpotAsPriceGo',
      spotDuration: 0,
      allocatePublicIp: true,
      internetMaxBandwidthOut: 1,
      internetChargeType: 'PayByTraffic',
      userData: ros.Fn.replace({ NOTIFY: ecsWaitConditionHandle.attrCurlCli }, specStartScript),
    });

    const serverInstance = new ecs.Instance(this, 'hcache-server', {
      vpcId: vpc.attrVpcId,
      keyPairName: keyPair.attrKeyPairName,
      vSwitchId: vswitch.attrVSwitchId,
      imageId: ecsImageId,
      securityGroupId: sg.attrSecurityGroupId,
      instanceType: 'ecs.g7.xlarge',
      instanceName: 'hcache-benchmark-server',
      systemDiskCategory: ecsSystemDiskCategory,
      password: ecsPassword,
      spotStrategy: 'SpotAsPriceGo',
      spotDuration: 0,
      allocatePublicIp: false,
      internetMaxBandwidthOut: 0,
      internetChargeType: 'PayByTraffic',
      userData: ros.Fn.replace({ NOTIFY: ecsWaitConditionHandle.attrCurlCli }, specStartScript),
    });

    new ros.RosOutput(this, 'instance_id', { value: clientInstance.attrInstanceId });
    new ros.RosOutput(this, 'private_ip', { value: clientInstance.attrPrivateIp });
    new ros.RosOutput(this, 'public_ip', { value: clientInstance.attrPublicIp });
    new ros.RosOutput(this, 'hcache_ip', { value: serverInstance.attrPrivateIp });
  }
}
