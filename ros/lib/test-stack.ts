import * as ros from '@alicloud/ros-cdk-core';
import * as ecs from '@alicloud/ros-cdk-ecs';
import * as ROS from '@alicloud/ros-cdk-ros';
import { readFileSync } from 'fs';
import { hostname } from 'os';

const yumInstallPackages = `#!/bin/bash
  yum makecache --refresh
  yum install -y ccache curl make g++ gcc git clang-devel htop nfs-utils tmux openssl-devel perf > ~/yum.log
`

const aptInstallPackages = `#!/bin/bash
   mv /etc/apt/sources.list /etc/apt/sources.list.bak  
      cat <<EOF > /etc/apt/sources.list
deb http://mirrors.cloud.aliyuncs.com/debian/ testing main
deb-src http://mirrors.cloud.aliyuncs.com/debian/ testing main
EOF
      export DEBIAN_FRONTEND=noninteractive
      apt-get update 
      apt-get install -y ccache libzstd-dev libdouble-conversion-dev libgoogle-glog-dev build-essential curl git libclang-dev htop nfs-common tmux linux-perf cmake libssl-dev > ~/apt.log
      apt-get autoremove -y
`


const startupScriptFromCleanImage = `
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly > ~/rustup.log
      mkdir -p ~/.cargo
      cat <<EOF > ~/.cargo/config
[source.crates-io]
replace-with = 'tuna'

[source.tuna]
registry = "https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index.git"
EOF
      # Update crates.io index in background
      cd /tmp && cargo install lazy_static &
      cat <<EOF | sudo tee -a /etc/security/limits.conf 
* hard memlock unlimited
* soft memlock unlimited
root hard nofile 1000000
root soft nofile 1000000
* hard nofile 1000000
* soft nofile 1000000
EOF
      cat <<EOF | sudo tee -a /etc/sysctl.conf
net.ipv4.ip_local_port_range = 1024 65535
net.ipv4.ip_local_reserved_ports = 8080
net.ipv4.tcp_fin_timeout = 15
net.ipv4.tcp_tw_reuse = 1
net.core.somaxconn = 32768
net.ipv4.tcp_max_tw_buckets = 30000
net.ipv4.tcp_sack = 1
kernel.perf_event_paranoid = 1
fs.aio-max-nr = 1048576
EOF

        # 自动重启脚本
        cat <<EOF > ~/auto-restart.sh
#!/bin/bash
ulimit -n 1000000
export INIT_DIR=/data
while true; do
  /usr/bin/hcache --reactor-backend io_uring
done
EOF
        # 启动脚本
        cat <<EOF > ~/start.sh
#!/bin/bash
export THREAD_COUNT=\\$(nproc)
cd ~ && nohup ~/auto-restart.sh 2>&1 &
EOF
      ln -s /usr/bin/ccache /usr/local/bin/gcc
      ln -s /usr/bin/ccache /usr/local/bin/g++
      chmod +x ~/start.sh
      chmod +x ~/auto-restart.sh
      sysctl -p
      NOTIFY
      `;
const imageAndStartScript = {
  "alinux": {
    startScript: `${yumInstallPackages}
    ${startupScriptFromCleanImage}`,
    imageId: "aliyun_3_x64_20G_alibase_20220527.vhd",
  },
  "debian": {
    startScript: `${aptInstallPackages}
    ${startupScriptFromCleanImage}`,
    imageId: "debian_11_3_x64_20G_alibase_20220531.vhd",
  },
  "custom": {
    imageId: "m-2ze6tbibqok06pny2wx3",
    startScript: `#!/bin/bash
    NOTIFY
    `,
  }
}

export class TestStack extends ros.Stack {
  constructor(scope: ros.Construct, id: string, props?: ros.StackProps) {
    super(scope, id, props);
    new ros.RosInfo(this, ros.RosInfo.description, "部署一个测试用的和用来打包镜像的按量付费 ECS");
    // The code that defines your stack goes here

    // 指定使用的镜像和启动脚本
    const fromWhich = "debian";
    const spec = imageAndStartScript[fromWhich];
    const specImageId = spec.imageId;
    const specStartScript = spec.startScript;

    // 随机选择一个可用区部署
    const zoneId = ros.Fn.select(0, ros.Fn.getAzs(ros.RosPseudo.region));

    // 创建虚拟网络
    // 构建 VPC
    const vpc = new ecs.Vpc(this, 'hcache-vpc', {
      vpcName: 'hcache-vpc',
      cidrBlock: '10.0.0.0/8',
      description: 'hcache vpc'
    });

    // 构建 VSwitch
    const vswitch = new ecs.VSwitch(this, 'hcache-vswitch', {
      vpcId: vpc.attrVpcId,
      zoneId: zoneId,
      vSwitchName: 'hcache-vswitch',
      cidrBlock: '10.1.1.0/24',
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
      defaultValue: "ecs.c7.4xlarge",
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
      count: 1
    });

    const ecsWaitCondition = new ROS.WaitCondition(this, 'RosWaitCondition', {
      timeout: 600,
      handle: ros.Fn.ref('RosWaitConditionHandle'),
      count: 1
    });

    const ecsInstance = new ecs.Instance(this, 'hcache-test', {
      vpcId: vpc.attrVpcId,
      keyPairName: keyPair.attrKeyPairName,
      vSwitchId: vswitch.attrVSwitchId,
      imageId: ecsImageId,
      securityGroupId: sg.attrSecurityGroupId,
      instanceType: ecsInstanceType,
      instanceName: 'hcahce-test-ecs',
      systemDiskCategory: ecsSystemDiskCategory,
      password: ecsPassword,
      spotStrategy: 'SpotAsPriceGo',
      spotDuration: 0,
      allocatePublicIp: true,
      internetMaxBandwidthOut: 1,
      internetChargeType: 'PayByTraffic',
      userData: ros.Fn.replace({ NOTIFY: ecsWaitConditionHandle.attrCurlCli }, specStartScript),
    });

    new ros.RosOutput(this, 'instance_id', { value: ecsInstance.attrInstanceId });
    new ros.RosOutput(this, 'private_ip', { value: ecsInstance.attrPrivateIp });
    new ros.RosOutput(this, 'public_ip', { value: ecsInstance.attrPublicIp });
  }
}
