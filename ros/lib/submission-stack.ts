import * as ros from '@alicloud/ros-cdk-core';
import * as ecs from "@alicloud/ros-cdk-ecs";
import * as ROS from '@alicloud/ros-cdk-ros';
import * as vpc from '@alicloud/ros-cdk-vpc';

export class SubmissionStack extends ros.Stack {
  constructor(scope: ros.Construct, id: string, props?: ros.StackProps) {
    super(scope, id, props);
    new ros.RosInfo(this, ros.RosInfo.description, "Tianchi Distributed Cache Service Submission Stack");
    // The code that defines your stack goes here
    const vpcId = new ros.RosParameter(this, "vpc_id", {
      type: ros.RosParameterType.STRING,
      associationProperty: "ALIYUN::ECS::VPC::VPCId",
    });
    const vswitchZoneId = new ros.RosParameter(this, "vswitch_zone_id", {
      type: ros.RosParameterType.STRING,
      associationProperty: "ALIYUN::ECS::Instance::ZoneId",
    });
    const vSwitchId = new ros.RosParameter(this, "vswitch_id", {
      type: ros.RosParameterType.STRING, 
      associationProperty: "ALIYUN::ECS::VSwitch::VSwitchId",
      associationPropertyMetadata: {
        "ZoneId": "vswitch_zone_id",
        "VpcId": "vpc_id",
      }
    });
    const securityGroupId = new ros.RosParameter(this, "security_group_id", {
      type: ros.RosParameterType.STRING,
      associationProperty: "ALIYUN::ECS::SecurityGroup::SecurityGroupId",
      associationPropertyMetadata: {
        "VpcId": "vpc_id",
      }
    });
    const ecsPassword = new ros.RosParameter(this, "ecs_password", {
      type: ros.RosParameterType.STRING,
      noEcho: true, minLength: 8, maxLength: 30,
      allowedPattern: "[0-9A-Za-z\\_\\-\\&:;'<>,=%`~!@#\\(\\)\\$\\^\\*\\+\\|\\{\\}\\[\\]\\.\\?\\/]+$"
    });
    const ecsInstanceType = new ros.RosParameter(this, "ecs_instance_type", {
      type: ros.RosParameterType.STRING,
      defaultValue: "ecs.c6.xlarge",
      associationProperty: "ALIYUN::ECS::Instance::InstanceType",
      associationPropertyMetadata: {
        "ZoneId": "vswitch_zone_id",
      },
    });
    const ecsImageId = new ros.RosParameter(this, "ecs_image_id", {
      type: ros.RosParameterType.STRING,
      defaultValue: "m-2ze5iosdwq4l46ldhuwt",
    });
    const ecsSystemDiskCategory = new ros.RosParameter(this, "ecs_system_disk_category", {
      type: ros.RosParameterType.STRING,
      defaultValue: "cloud_efficiency",
    });
    const nasUrl = new ros.RosParameter(this, "nas_url", {
      type: ros.RosParameterType.STRING,
    });
    const ecsWaitConditionHandle = new ROS.WaitConditionHandle(this, 'RosWaitConditionHandle', {
      count: 1
    });
    const ecsWaitCondition = new ROS.WaitCondition(this, 'RosWaitCondition', {
      timeout: 300,
      handle: ros.Fn.ref('RosWaitConditionHandle'),
      count: 1
    });
    const ecsGroups = new ecs.InstanceGroup(this, 'EcsService', {
      vpcId: vpcId,
      vSwitchId: vSwitchId,
      imageId: ecsImageId,
      maxAmount: 1,
      securityGroupId: securityGroupId,
      instanceType: ecsInstanceType,
      systemDiskCategory: ecsSystemDiskCategory,
      spotStrategy: "SpotAsPriceGo",
      instanceName: 'cache-service',
      allocatePublicIp: false,
      password: ecsPassword,
      userData: ros.Fn.replace(
        { "ros-notify": ecsWaitConditionHandle.attrCurlCli },
        `#!/bin/bash
        export nas_url=${nasUrl.valueAsString}
        sudo apt-get install -y nfs-common
        sudo mkdir /data2
        sudo mount -t nfs -o vers=3,nolock,proto=tcp,rsize=1048576,wsize=1048576,hard,timeo=600,retrans=2,noresvport "\${nas_url}" /data2
        cp /data2/* /data/

        # 启动服务
        cat <<EOF > ~/start.sh
#!/bin/bash
export THREAD_COUNT=$(nproc)
cd ~ && nohup hcache/target/release/hcache /data 2>&1 &
EOF
        chmod +x ~/start.sh
        cd ~ && bash start.sh

        ros-notify`
      ),
    });
    new ros.RosOutput(this, 'instance_id', { value: ros.Fn.select(0, ecsGroups.getAtt('InstanceIds')) });
    new ros.RosOutput(this, 'private_ip', { value: ros.Fn.select(0, ecsGroups.getAtt('PrivateIps')) });
    new ros.RosOutput(this, 'public_ip', { value: ros.Fn.select(0, ecsGroups.getAtt('PublicIps')) });
  }
}
