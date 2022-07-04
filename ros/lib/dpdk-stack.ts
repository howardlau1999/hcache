import * as ros from '@alicloud/ros-cdk-core';
import * as ecs from '@alicloud/ros-cdk-ecs';
import * as ROS from '@alicloud/ros-cdk-ros';
import { readFileSync } from 'fs';
import { hostname } from 'os';
import { aptInstallPackages } from './test-stack';

export class DPDKStack extends ros.Stack {
  constructor(scope: ros.Construct, id: string, props?: ros.StackProps) {
    super(scope, id, props);
    new ros.RosInfo(this, ros.RosInfo.description, "部署两台 ECS，每台都有两张网卡，方便测试，一张用来 SSH，另一张用来 DPDK");
    // The code that defines your stack goes here

    // 随机选择一个可用区部署
    const zoneId = ros.Fn.select(0, ros.Fn.getAzs(ros.RosPseudo.region));

    // 创建虚拟网络
    // 构建 VPC
    const ecsVpc = new ecs.Vpc(this, 'hcache-vpc', {
      vpcName: 'hcache-dpdk-vpc',
      cidrBlock: '192.168.0.0/16',
      description: 'hcache dpdk vpc'
    });

    // 构建 VSwitch
    const ecsvSwitch = new ecs.VSwitch(this, 'hcache-dpdk-vswitch', {
      vpcId: ecsVpc.attrVpcId,
      zoneId: zoneId,
      vSwitchName: 'hcache-dpdk-vswitch',
      cidrBlock: '192.168.1.0/24',
    });

    // 指定系统镜像、系统密码、实例类型
    const ecsPassword = new ros.RosParameter(this, "ecs_password", {
      type: ros.RosParameterType.STRING,
      noEcho: true, minLength: 8, maxLength: 30,
      allowedPattern: "[0-9A-Za-z\\_\\-\\&:;'<>,=%`~!@#\\(\\)\\$\\^\\*\\+\\|\\{\\}\\[\\]\\.\\?\\/]+$",
      defaultValue: "hcache@2022",
    });
    const ecsInstanceType = new ros.RosParameter(this, "ecs_instance_type", {
      type: ros.RosParameterType.STRING,
      defaultValue: "ecs.c6.xlarge",
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
    const sg = new ecs.SecurityGroup(this, 'hcahce-sg', { vpcId: ecsVpc.attrVpcId });

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

    // 用于服务器之间互联的密钥对
    const serverKey = new ecs.SSHKeyPair(this, 'hcache-dpdk-server-key', {
      keyPairName: `hcache-dpdk-server-key`,
      tags: [{ key: 'hcache', value: 'dpdk' }],
    });

    // 等待逻辑，用于等待 ECS 中应用安装完成
    const serverCount = 2;
    const ecsWaitConditionHandle = new ROS.WaitConditionHandle(this, 'RosWaitConditionHandle', {
      count: serverCount
    });

    const ecsWaitCondition = new ROS.WaitCondition(this, 'RosWaitCondition', {
      timeout: 600,
      handle: ros.Fn.ref('RosWaitConditionHandle'),
      count: serverCount
    });

    const servers = [];
    const nics = [];
    for (let i = 0; i < serverCount; i++) {
      const serverInstance = new ecs.Instance(this, `hcache-dpdk-${i}`, {
        hostName: `hcache-dpdk-node-${i}`,
        vpcId: ecsVpc.attrVpcId,
        keyPairName: serverKey.attrKeyPairName,
        vSwitchId: ecsvSwitch.attrVSwitchId,
        imageId: "debian_11_3_x64_20G_alibase_20220531.vhd",
        securityGroupId: sg.attrSecurityGroupId,
        instanceType: i === 0 ? 'ecs.c7.4xlarge' : ecsInstanceType,
        instanceName: `hcache-dpdk-${i}`,
        systemDiskCategory: ecsSystemDiskCategory,
        password: ecsPassword,
        spotStrategy: 'SpotAsPriceGo',
        spotDuration: 0,
        allocatePublicIp: i === 0,
        internetMaxBandwidthOut: i === 0 ? 1 : 0,
        internetChargeType: 'PayByTraffic',
        userData: ros.Fn.replace({
          NOTIFY: ecsWaitConditionHandle.attrCurlCli,
          SSH_PRIVATE_KEY: serverKey.attrPrivateKeyBody,
          SSH_PUBLIC_KEY: pubKey,
        }, `#!/bin/bash
        ${aptInstallPackages}
        apt-get install -y python3-pyelftools libnuma-dev meson libpcap-dev ninja-build
        mkdir -p ~/.ssh
        cat <<EOF > ~/.ssh/id_rsa
SSH_PRIVATE_KEY
EOF
        cat <<EOF >> ~/.ssh/authorized_keys
    
SSH_PUBLIC_KEY
EOF
        chmod 600 ~/.ssh/id_rsa
        chmod 600 ~/.ssh/authorized_keys
        ln -s /usr/bin/ccache /usr/bin/gcc
        ln -s /usr/bin/ccache /usr/bin/g++
      NOTIFY
        `),
      });
      const nic = new ecs.NetworkInterface(this, `hcache-dpdk-nic-${i}`, {
        vSwitchId: ecsvSwitch.attrVSwitchId,
        networkInterfaceName: `hcache-dpdk-nic-${i}`,
        securityGroupId: sg.attrSecurityGroupId,
      });
      const nicAttachment = new ecs.NetworkInterfaceAttachment(this, `hcache-dpdk-nic-attachment-${i}`, {
        instanceId: serverInstance.attrInstanceId,
        networkInterfaceId: nic.attrNetworkInterfaceId,
      });
      servers.push(serverInstance);
      nics.push(nic);
    }
    const hostsCommand = new ecs.RunCommand(this, 'hcache-dpdk-hosts', {
      commandContent: ros.Fn.replace({ SERVERS: ros.Fn.join('\n', servers.map((server, i) => `${server.attrPrivateIp} node-${i} ${server.attrHostName}`)) }, `
      cat <<EOF > /root/servers
SERVERS
EOF
      ssh-keyscan -H -f /root/servers >> ~/.ssh/known_hosts
      cat /root/servers >> /etc/hosts
      `),
      type: 'RunShellScript',
      instanceIds: servers.map((server) => server.attrInstanceId),
    });
    hostsCommand.addDependency(ecsWaitCondition);

    new ros.RosOutput(this, 'instance_id', { value: servers.map((server) => server.attrInstanceId) });
    new ros.RosOutput(this, 'private_ip', { value: servers.map((server) => server.attrPrivateIp) });
    new ros.RosOutput(this, 'nic_ip', { value: nics.map((nic) => nic.attrPrivateIpAddress) });
    new ros.RosOutput(this, 'public_ip', { value: servers[0].attrPublicIp });
  }
}
