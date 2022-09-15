import * as ros from '@alicloud/ros-cdk-core';
import * as ecs from '@alicloud/ros-cdk-ecs';
import * as ROS from '@alicloud/ros-cdk-ros';
import { readFileSync } from 'fs';
import { hostname } from 'os';

export const dnfInstallPackages = `#!/bin/bash
  dnf -y install gcc-c++ snappy-devel glog-devel jsoncpp-devel  ninja-build  libzstd-devel ragel    boost-devel    fmt-devel    libubsan    libasan    libatomic\
    git ccache curl make gcc cmake clang-devel htop nfs-utils tmux openssl-devel perf hwloc-devel\
    numactl-devel  libpciaccess-devel    cryptopp-devel    libxml2-devel    xfsprogs-devel    gnutls-devel    lksctp-tools-devel    lz4-devel\
    meson    python3    python3-pyelftools   systemtap-sdt-devel   libtool    yaml-cpp-devel    c-ares-devel    stow\
    diffutils    openssl    boost-devel   libtool-ltdl-devel trousers-devel libidn2-devel libunistring-devel > ~/dnf.log
` 

const yumInstallPackages = `#!/bin/bash
  yum makecache --refresh
  yum install -y ccache curl cmake make g++ gcc git clang-devel htop nfs-utils tmux openssl-devel perf > ~/yum.log
`

export const aptInstallPackages = `#!/bin/bash
#    mv /etc/apt/sources.list /etc/apt/sources.list.bak  
#      cat <<EOF > /etc/apt/sources.list
# deb http://mirrors.cloud.aliyuncs.com/debian/ testing main
# deb-src http://mirrors.cloud.aliyuncs.com/debian/ testing main
# EOF
      export DEBIAN_FRONTEND=noninteractive
      apt-get update 
      while true; do
        apt-get install -y pkg-config ccache python3-pyelftools meson libpcap-dev ninja-build distcc libjsoncpp-dev libboost-all-dev libzstd-dev libdouble-conversion-dev systemtap-sdt-dev libgoogle-glog-dev \
          build-essential curl git libclang-dev xfslibs-dev htop nfs-common tmux cmake libssl-dev libssl3 \
          libxml2-dev libyaml-cpp-dev libc-ares-dev libzstd-dev libsnappy-dev liblz4-dev libgnutls28-dev libhwloc-dev libnuma-dev libpciaccess-dev libcrypto++-dev libicu70=70.1-2 >> ~/apt.log
        if [ $? -eq 0 ]; then
          break
        fi
      done
      apt-get autoremove -y
      ln -s /usr/bin/ccache /usr/local/bin/gcc
      ln -s /usr/bin/ccache /usr/local/bin/g++
`

export const installRust = `mkdir -p ~/.cargo

cat <<EOF > ~/.cargo/config
[source.crates-io]
replace-with = 'tuna'

[source.tuna]
registry = "https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index.git"
EOF

bash -c "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly && . $HOME/.cargo/env && cd /tmp && cargo install lazy_static" > ~/rustup.log & 
`

export const adjustLimits = `
cat <<EOF | sudo tee -a /etc/security/limits.conf 
* hard memlock unlimited
* soft memlock unlimited
root hard nofile 1000000
root soft nofile 1000000
* hard nofile 1000000
* soft nofile 1000000
EOF
`

export const adjustSysctl = `
cat <<EOF | sudo tee -a /etc/sysctl.conf
vm.dirty_ratio=80
net.core.busy_poll=1
# net.ipv4.tcp_congestion_control=reno
net.ipv4.ip_local_port_range = 1024 65535
net.ipv4.ip_local_reserved_ports = 8080,58080
net.ipv4.tcp_fin_timeout = 15
net.ipv4.tcp_tw_reuse = 1
net.core.somaxconn = 32768
net.ipv4.tcp_max_tw_buckets = 30000
net.ipv4.tcp_sack = 1
kernel.perf_event_paranoid = 1
fs.aio-max-nr = 1048576
EOF
sysctl -p
`

export const uninstallAegis = `
# curl -LO http://update.aegis.aliyun.com/download/uninstall.sh
# chmod +x uninstall.sh
# ./uninstall.sh
# curl -LO http://update.aegis.aliyun.com/download/quartz_uninstall.sh
# chmod +x quartz_uninstall.sh
# ./quartz_uninstall.sh
`

export const disableSpectre = `
sed -i 's/^GRUB_CMDLINE_LINUX="/&nospectre_v1 nospectre_v2 pti=off mds=off tsx_async_abort=off /'  /etc/default/grub
update-grub
if [ $? -ne 0 ]; then
  grub2-mkconfig -o /boot/grub2/grub.cfg
fi
`

const startupScriptFromCleanImage = `
      ${installRust}
      ${adjustLimits}
      ${adjustSysctl}
      ${uninstallAegis}
      ${disableSpectre}

        # 自动重启脚本
        cat <<EOF > ~/auto-restart.sh
#!/bin/bash
export INIT_DIRS=/init_data/data1,/init_data/data2,/init_data/data3
while true; do
  /usr/bin/hcache --reserve-memory 512M
done
EOF
        # 启动脚本
        cat <<EOF > ~/start.sh
#!/bin/bash
modprobe -rv ip_tables
# dhclient -x -pf /var/run/dhclient-eth0.pid
# ip addr change \\$( ip -4 addr show dev eth0 | grep 'inet' | awk '{ print \\$2 " brd " \\$4 " scope global"}') dev eth0 valid_lft forever preferred_lft forever
# export TXQUEUES=(\\$(ls -1qdv /sys/class/net/eth0/queues/tx-*))
# for i in \\\${!TXQUEUES[@]}; do printf '%x' $((2**i)) > \\\${TXQUEUES[i]}/xps_cpus; done;
cd ~ && nohup ~/auto-restart.sh 2>&1 &
EOF

      chmod +x ~/start.sh
      chmod +x ~/auto-restart.sh
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
  "ubuntu": {
    startScript: `#!/bin/bash
      export DEBIAN_FRONTEND=noninteractive
      apt-get update && apt-get install --allow-downgrades -y curl git  nfs-common  pkg-config ccache python3-pyelftools meson libpcap-dev ninja-build distcc libjsoncpp-dev libboost-all-dev libzstd-dev libdouble-conversion-dev systemtap-sdt-dev libgoogle-glog-dev \
          build-essential curl git libclang-dev xfslibs-dev htop nfs-common tmux cmake libssl-dev libssl3 \
          libxml2-dev libyaml-cpp-dev libc-ares-dev libzstd-dev libsnappy-dev liblz4-dev libgnutls28-dev libhwloc-dev libnuma-dev libpciaccess-dev libcrypto++-dev libicu70=70.1-2
      ${adjustLimits}
      ${adjustSysctl}
      ${uninstallAegis}
      ${disableSpectre}

        # 自动重启脚本
        cat <<EOF > ~/auto-restart.sh
#!/bin/bash
export INIT_DIRS=/init_data/data1,/init_data/data2,/init_data/data3
while true; do
  ulimit -l unlimited
  /usr/bin/hcache --reserve-memory 512M
done
EOF
        # 启动脚本
        cat <<EOF > ~/start.sh
#!/bin/bash
modprobe -rv ip_tables
# dhclient -x -pf /var/run/dhclient-eth0.pid
# ip addr change \\$( ip -4 addr show dev eth0 | grep 'inet' | awk '{ print \\$2 " brd " \\$4 " scope global"}') dev eth0 valid_lft forever preferred_lft forever
# export TXQUEUES=(\\$(ls -1qdv /sys/class/net/eth0/queues/tx-*))
# for i in \\\${!TXQUEUES[@]}; do printf '%x' $((2**i)) > \\\${TXQUEUES[i]}/xps_cpus; done;
ulimit -l unlimited
cd ~ && nohup ~/auto-restart.sh 2>&1 &
EOF

      chmod +x ~/start.sh
      chmod +x ~/auto-restart.sh
      NOTIFY
  `,
    imageId: "ubuntu_22_04_x64_20G_alibase_20220803.vhd",
  },
  "fedora": {
    imageId: "fedora_35_x64_20G_alibase_20220531.vhd",
    startScript: `${dnfInstallPackages}
    ${startupScriptFromCleanImage}
    `
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
    const fromWhich = process.env.IMAGE_FROM || "ubuntu";
    const spec = (imageAndStartScript as any)[fromWhich];
    const specImageId = spec.imageId;
    const specStartScript = spec.startScript;

    // 随机选择一个可用区部署
    const zoneId = 'cn-beijing-a';

    // 创建虚拟网络
    // 构建 VPC
    const ecsVpc = new ecs.Vpc(this, 'hcache-vpc', {
      vpcName: 'hcache-vpc',
      cidrBlock: '10.0.0.0/8',
      description: 'hcache vpc'
    });

    // 构建 VSwitch
    const ecsvSwitch = new ecs.VSwitch(this, 'hcache-vswitch', {
      vpcId: ecsVpc.attrVpcId,
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
      defaultValue: "ecs.c6.4xlarge",
      associationProperty: "ALIYUN::ECS::Instance::InstanceType",
      associationPropertyMetadata: {
        "ZoneId": zoneId,
      },
    });
    const ecsSystemDiskCategory = new ros.RosParameter(this, "ecs_system_disk_category", {
      type: ros.RosParameterType.STRING,
      defaultValue: "cloud_efficiency",
    });

    // 创建安全组开放端口
    const sg = new ecs.SecurityGroup(this, 'hcache-sg', { vpcId: ecsVpc.attrVpcId });

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
    const ecsKeyPair = new ecs.SSHKeyPair(this, 'hcache-key-pair', {
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
      vpcId: ecsVpc.attrVpcId,
      keyPairName: ecsKeyPair.attrKeyPairName,
      vSwitchId: ecsvSwitch.attrVSwitchId,
      imageId: ecsImageId,
      securityGroupId: sg.attrSecurityGroupId,
      instanceType: ecsInstanceType,
      instanceName: 'hcache-test-ecs',
      hostName: 'hcache-build',
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
