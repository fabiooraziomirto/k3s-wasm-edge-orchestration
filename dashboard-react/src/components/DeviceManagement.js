import React, { useState, useEffect } from 'react';
import {
  Table,
  Card,
  Button,
  Space,
  Tag,
  Modal,
  Form,
  Input,
  Select,
  Popconfirm,
  Typography,
  Row,
  Col,
  Statistic,
  Tooltip,
  App,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  ClockCircleOutlined,
  QuestionCircleOutlined,
  UserAddOutlined,
  LinkOutlined,
  DisconnectOutlined,
  PlayCircleOutlined,
  PauseCircleOutlined,
} from '@ant-design/icons';
import { apiGet, apiPost, apiDelete } from '../utils/api';

const { Title } = Typography;
const { Option } = Select;

const DeviceManagement = () => {
  const { message } = App.useApp(); // Use App.useApp() instead of static message
  const [devices, setDevices] = useState([]);
  const [gateways, setGateways] = useState([]);
  const [publicKeyModalVisible, setPublicKeyModalVisible] = useState(false);
  const [selectedPublicKey, setSelectedPublicKey] = useState('');
  const [loading, setLoading] = useState(false);
  const [creatingDevice, setCreatingDevice] = useState(false);
  const [connectingDevice, setConnectingDevice] = useState(new Set());
  const [modalVisible, setModalVisible] = useState(false);
  const [form] = Form.useForm();

  // Initialize devices and gateways only once
  useEffect(() => {
    fetchDevices();
    fetchGateways();
  }, []);

  const fetchDevices = async () => {
    setLoading(true);
    try {
      const data = await apiGet('/api/v1/devices', 10000);
      let deviceList = data.devices || [];
      
      // Normalize device data - ensure publicKey is available
      deviceList = deviceList.map(device => ({
        ...device,
        // Ensure publicKey is set (try both camelCase and snake_case)
        publicKey: device.publicKey || device.public_key || null,
        // Ensure id is set
        id: device.id || device.device_id,
        // Ensure name is set
        name: device.name || device.device_id || device.id,
        // Ensure device_id is set for rowKey
        device_id: device.device_id || device.id,
      }));
      
      // Use real data from backend - no mock data
      setDevices(deviceList);
    } catch (error) {
      console.error('Error fetching devices:', error);
    } finally {
      setLoading(false);
    }
  };

  const fetchGateways = async () => {
    try {
      const data = await apiGet('/api/v1/gateways', 10000);
      let gatewayList = data.gateways || [];
      setGateways(gatewayList);
    } catch (error) {
      console.error('Error fetching gateways:', error);
    }
  };

  const handleCreateDevice = async (values) => {
    setCreatingDevice(true);
    try {
      // Auto-generate name if not provided
      const deviceCount = devices.length + 1;
      const deviceName = values.name || `device-${values.gateway || 'gateway-1'}-${deviceCount}`;
      
      const response = await apiPost('/api/v1/devices', {
        name: deviceName,
        type: 'MCU', // Fixed to MCU only
        architecture: 'ARM_CORTEX_M', // Fixed architecture for QEMU
        mcuType: values.mcuType || 'Stm32F746gDisco', // MCU type selection (default to STM32F746G Discovery with Ethernet)
        gatewayId: values.gateway || 'gateway-1', // Use selected gateway
        qemuEnabled: true // Enable QEMU emulation
      }, 60000); // Increased timeout to 60 seconds for device creation (gateway + K8s CRD creation)
      
      // Check if creation was successful
      if (response?.success === false || response?.errors?.length > 0) {
        const errorMsg = response?.message || response?.errors?.join('; ') || 'Failed to create device';
        message.error(`Device creation failed: ${errorMsg}`);
        
        // Check if it's a gateway connection error
        if (errorMsg.includes('Connection refused') || errorMsg.includes('localhost:8080') || errorMsg.includes('Cannot connect to gateway')) {
          message.warning('Gateway connection failed. This is expected if device enrollment API is not working. The device can still be created via kubectl.');
        }
        
        console.error('Device creation failed:', response);
        setModalVisible(false);
        form.resetFields();
        return;
      }
      
      // Refresh the devices list to get the updated data
      await fetchDevices();
      message.success(response?.message || 'Device created successfully');
      console.log('Device created successfully:', response?.message || 'Device created');
      
      // Close modal and reset form on success
      setModalVisible(false);
      form.resetFields();
    } catch (error) {
      console.error('Error creating device:', error);
      const errorMsg = error.message || 'Unknown error occurred';
      message.error(`Failed to create device: ${errorMsg}`);
      
      // Check if it's a gateway connection error
      if (errorMsg.includes('Connection refused') || errorMsg.includes('localhost:8080') || errorMsg.includes('Cannot connect to gateway')) {
        message.warning('Gateway connection failed. This is expected if device enrollment API is not working. The device can still be created via kubectl.');
      }
      
      // Still close modal on error/timeout so user can try again
      setModalVisible(false);
      form.resetFields();
    } finally {
      setCreatingDevice(false);
    }
  };

  const handleDeleteDevice = async (deviceId) => {
    try {
      await apiDelete(`/api/v1/devices/${deviceId}`, 10000);
      setDevices(prevDevices => prevDevices.filter(device => device.id !== deviceId));
      console.log('Device deleted successfully:', deviceId);
    } catch (error) {
      console.error('Error deleting device:', error);
    }
  };

  const handleEnrollDevice = async (deviceId) => {
    try {
      // First, get available gateways
      const gateways = await apiGet('/api/v1/gateways', 10000);
      const availableGateways = gateways.gateways?.filter(g => g.status === 'Running') || [];
      
      if (availableGateways.length === 0) {
        console.error('No active gateways available for enrollment');
        return;
      }
      
      // For now, select the first available gateway
      // In a real implementation, this could be a user selection
      const selectedGateway = availableGateways[0];
      
      const data = await apiPost(`/api/v1/devices/${deviceId}/enroll`, {
        gatewayId: selectedGateway.id,
        gatewayName: selectedGateway.name
      }, 15000);
      
      setDevices(prevDevices => 
        prevDevices.map(device => 
          device.id === deviceId 
            ? { ...device, enrolled: true, status: 'Enrolled', gatewayId: data.gatewayId, gatewayName: data.gatewayName }
            : device
        )
      );
      console.log(`Device ${deviceId} enrolled successfully to gateway ${selectedGateway.name}`);
    } catch (error) {
      console.error('Error enrolling device:', error);
    }
  };

  const handleConnectDevice = async (deviceId) => {
    setConnectingDevice(prev => new Set(prev).add(deviceId));
    try {
      // Increased timeout to 90 seconds for connection (Renode startup + firmware connection + TLS handshake)
      const data = await apiPost(`/api/v1/devices/${deviceId}/connect`, {}, 90000);
      
      if (data && data.success) {
        // Refresh the devices list to get the updated status
        await fetchDevices();
        console.log(`Device ${deviceId} connected successfully`);
      } else {
        // Still refresh to show updated status even if response is unclear
        await fetchDevices();
        console.log(`Device ${deviceId} connection initiated`);
      }
    } catch (error) {
      console.error('Error connecting device:', error);
      // Don't show error if it's just a timeout - connection might still be in progress
      if (error.message && error.message.includes('timeout')) {
        console.warn(`Device ${deviceId} connection is taking longer than expected. Please check the device status.`);
      }
      // Refresh anyway to show current status
      await fetchDevices();
    } finally {
      setConnectingDevice(prev => {
        const newSet = new Set(prev);
        newSet.delete(deviceId);
        return newSet;
      });
    }
  };

  const handleDisconnectDevice = async (deviceId) => {
    try {
      const data = await apiPost(`/api/v1/devices/${deviceId}/disconnect`, {}, 10000);
      
      setDevices(prevDevices => 
        prevDevices.map(device => 
          device.id === deviceId 
            ? { ...device, status: data.status || 'Disconnected', connected: false }
            : device
        )
      );
      console.log(`Device ${deviceId} disconnected successfully`);
    } catch (error) {
      console.error('Error disconnecting device:', error);
    }
  };

  const handleStartRenode = async (deviceId) => {
    try {
      message.info(`Starting emulation for device ${deviceId}...`);
      const data = await apiPost(`/api/v1/devices/${deviceId}/emulation/start`, {}, 30000);
      
      if (data && data.success) {
        message.success(`Emulation started successfully for device ${deviceId}`);
        setDevices(prevDevices => 
          prevDevices.map(device => 
            device.id === deviceId 
              ? { ...device, emulationStatus: 'Running', renodeInstance: data.renodeInstance }
              : device
          )
        );
        console.log(`Renode Docker emulation started for device ${deviceId}`);
      } else {
        message.error(`Failed to start emulation for device ${deviceId}`);
      }
    } catch (error) {
      console.error('Error starting Renode emulation:', error);
      message.error(`Error starting emulation: ${error.message || 'Unknown error'}`);
    }
  };

  const handleStopRenode = async (deviceId) => {
    try {
      message.info(`Stopping emulation for device ${deviceId}...`);
      const data = await apiPost(`/api/v1/devices/${deviceId}/emulation/stop`, {}, 15000);
      
      if (data && data.success) {
        message.success(`Emulation stopped successfully for device ${deviceId}`);
        setDevices(prevDevices => 
          prevDevices.map(device => 
            device.id === deviceId 
              ? { ...device, emulationStatus: 'Stopped', renodeInstance: null }
              : device
          )
        );
        console.log(`Renode emulation stopped for device ${deviceId}`);
      }
    } catch (error) {
      console.error('Error stopping Renode emulation:', error);
      message.error(`Error stopping emulation: ${error.message || 'Unknown error'}`);
    }
  };

  const getStatusTag = (status) => {
    const statusConfig = {
      Connected: { color: 'green', icon: <CheckCircleOutlined /> },
      Enrolled: { color: 'blue', icon: <CheckCircleOutlined /> },
      Enrolling: { color: 'cyan', icon: <ClockCircleOutlined /> },
      Pending: { color: 'orange', icon: <ClockCircleOutlined /> },
      Disconnected: { color: 'gray', icon: <ExclamationCircleOutlined /> },
      Unreachable: { color: 'red', icon: <ExclamationCircleOutlined /> },
    };

    const config = statusConfig[status] || { color: 'default', icon: null };
    return (
      <Tag color={config.color} icon={config.icon}>
        {status}
      </Tag>
    );
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      sorter: (a, b) => (a.name || '').localeCompare(b.name || ''),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (status) => getStatusTag(status),
      filters: [
        { text: 'Connected', value: 'Connected', key: 'connected' },
        { text: 'Enrolled', value: 'Enrolled', key: 'enrolled' },
        { text: 'Enrolling', value: 'Enrolling', key: 'enrolling' },
        { text: 'Pending', value: 'Pending', key: 'pending' },
        { text: 'Disconnected', value: 'Disconnected', key: 'disconnected' },
        { text: 'Unreachable', value: 'Unreachable', key: 'unreachable' },
      ],
      onFilter: (value, record) => record.status === value,
    },
    {
      title: 'Architecture',
      dataIndex: 'architecture',
      key: 'architecture',
      render: (arch) => (
        <Tag color="blue">ARM Cortex-M (Renode)</Tag>
      ),
    },
    {
      title: 'Device Type',
      dataIndex: 'type',
      key: 'type',
      render: (type) => (
        <Tag color="green">{type}</Tag>
      ),
      filters: [
        { text: 'MCU', value: 'MCU', key: 'mcu' },
        { text: 'MPU', value: 'MPU', key: 'mpu' },
        { text: 'RISC-V', value: 'RISC-V', key: 'riscv' },
      ],
      onFilter: (value, record) => record.type === value,
    },
    {
      title: 'MCU Type',
      dataIndex: 'mcuType',
      key: 'mcuType',
      render: (mcuType) => {
        const mcuNames = {
          'Stm32F746gDisco': '🌐 STM32F746G Discovery',
          'FrdmK64f': '🌐 FRDM-K64F',
          'Esp32DevkitC': '📡 ESP32 DevKit C',
          'Nrf52840DK': '📡 nRF52840 DK',
          'Stm32F4Disco': 'STM32F4 Discovery',
          'RenodeArduinoNano33Ble': 'Arduino Nano 33 BLE (Legacy)',
          'RenodeStm32F4Discovery': 'STM32F4 Discovery (Legacy)',
          'Mps2An385': 'MPS2-AN385 (Legacy)'
        };
        return <Tag color="purple">{mcuNames[mcuType] || mcuType}</Tag>;
      },
    },
    {
      title: 'Gateway',
      dataIndex: 'gateway',
      key: 'gateway',
      render: (gateway) => (
        <Tag color="orange">{gateway}</Tag>
      ),
    },
    {
      title: 'Last Heartbeat',
      dataIndex: 'lastHeartbeat',
      key: 'lastHeartbeat',
      render: (timestamp) => timestamp ? new Date(timestamp * 1000).toLocaleString() : 'Never',
    },
    {
      title: 'Public Key',
      dataIndex: 'publicKey',
      key: 'publicKey',
      width: 150,
      render: (publicKey, record) => {
        if (!publicKey) {
          return <Tag color="default">Not set</Tag>;
        }
        return (
          <Tooltip title="Click to view public key">
            <Button
              type="link"
              size="small"
              onClick={() => {
                setSelectedPublicKey(publicKey);
                setPublicKeyModalVisible(true);
              }}
            >
              View Key
            </Button>
          </Tooltip>
        );
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 200,
      fixed: 'right',
      render: (_, record) => (
        <Space size="small">
          {record.status === 'Disconnected' && (
            <Tooltip key="enroll" title="Enroll device in the system">
              <Button 
                type="link" 
                icon={<UserAddOutlined />}
                size="small"
                onClick={() => handleEnrollDevice(record.id)}
              >
                Enroll
              </Button>
            </Tooltip>
          )}
          {record.status === 'Enrolled' && (
            <Tooltip key="connect" title="Connect device to gateway">
              <Button 
                type="link" 
                icon={<LinkOutlined />}
                size="small"
                onClick={() => handleConnectDevice(record.id)}
                loading={connectingDevice.has(record.id)}
                disabled={connectingDevice.has(record.id)}
              >
                {connectingDevice.has(record.id) ? 'Connecting...' : 'Connect'}
              </Button>
            </Tooltip>
          )}
          {record.status === 'Connected' && (
            <Tooltip key="disconnect" title="Disconnect device from gateway">
              <Button 
                type="link" 
                icon={<DisconnectOutlined />}
                size="small"
                onClick={() => handleDisconnectDevice(record.id)}
              >
                Disconnect
              </Button>
            </Tooltip>
          )}
          {record.emulationStatus === 'Not Started' || record.emulationStatus === 'Stopped' ? (
            <Tooltip key="start-renode" title="Start Renode emulation">
              <Button 
                type="link" 
                icon={<PlayCircleOutlined />}
                size="small"
                onClick={() => handleStartRenode(record.id)}
              >
                Start Renode
              </Button>
            </Tooltip>
          ) : (
            <Tooltip key="stop-renode" title="Stop Renode emulation">
              <Button 
                type="link" 
                icon={<PauseCircleOutlined />}
                size="small"
                onClick={() => handleStopRenode(record.id)}
              >
                Stop Renode
              </Button>
            </Tooltip>
          )}
          <Popconfirm
            key="delete"
            title="Are you sure you want to delete this device?"
            onConfirm={() => handleDeleteDevice(record.id)}
            okText="Yes"
            cancelText="No"
          >
            <Button 
              type="link" 
              danger 
              icon={<DeleteOutlined />}
              size="small"
            >
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const deviceStats = (Array.isArray(devices) ? devices : []).reduce(
    (acc, device) => {
      acc.total++;
      if (device.status === 'Connected') acc.connected++;
      else if (device.status === 'Enrolled') acc.enrolled++;
      else if (device.status === 'Enrolling') acc.enrolling++;
      else if (device.status === 'Pending') acc.pending++;
      else if (device.status === 'Disconnected') acc.disconnected++;
      else if (device.status === 'Unreachable') acc.unreachable++;
      return acc;
    },
    { total: 0, connected: 0, enrolled: 0, enrolling: 0, pending: 0, disconnected: 0, unreachable: 0 }
  );

  return (
    <div>
      <Title level={2}>Device Management</Title>
      
      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Total Devices"
              value={deviceStats.total}
              valueStyle={{ color: '#1890ff' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Connected"
              value={deviceStats.connected}
              valueStyle={{ color: '#3f8600' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Enrolled"
              value={deviceStats.enrolled}
              valueStyle={{ color: '#722ed1' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Enrolling"
              value={deviceStats.enrolling}
              valueStyle={{ color: '#13c2c2' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Disconnected"
              value={deviceStats.disconnected}
              valueStyle={{ color: '#8c8c8c' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Unreachable"
              value={deviceStats.unreachable}
              valueStyle={{ color: '#cf1322' }}
            />
          </Card>
        </Col>
      </Row>

      <Card>
        <div style={{ marginBottom: 16 }}>
          <Space>
            <Tooltip title="Create a new device with custom configuration">
              <Button
                type="primary"
                icon={<PlusOutlined />}
                onClick={() => setModalVisible(true)}
              >
                Add Device
              </Button>
            </Tooltip>
            <Tooltip title="Refresh the device list to get the latest status">
              <Button
                icon={<ReloadOutlined />}
                onClick={fetchDevices}
                loading={loading}
              >
                Refresh
              </Button>
            </Tooltip>
          </Space>
        </div>

        <Table
          columns={columns}
          dataSource={devices}
          rowKey={(record) => record.device_id || record.id || `device-${record.name || ''}`}
          loading={loading}
          pagination={{
            pageSize: 10,
            showSizeChanger: true,
            showQuickJumper: true,
            showTotal: (total, range) =>
              `${range[0]}-${range[1]} of ${total} devices`,
          }}
          scroll={{ x: 'max-content' }}
          size="small"
        />
      </Card>

      <Modal
        title="Create New Device"
        open={modalVisible}
        onCancel={() => {
          setModalVisible(false);
          form.resetFields();
        }}
        footer={null}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={handleCreateDevice}
        >
          <Form.Item
            name="name"
            label="Device Name (Optional)"
            help="Leave empty for auto-generated name"
          >
            <Input placeholder="Auto-generated if empty" />
          </Form.Item>

          <Form.Item
            name="architecture"
            label="Architecture"
            help="Fixed to ARM Cortex-M for Renode emulation"
          >
            <Select placeholder="ARM Cortex-M (Renode)" disabled>
              <Option value="ARM_CORTEX_M">ARM Cortex-M (Renode)</Option>
            </Select>
          </Form.Item>

          <Form.Item
            name="deviceType"
            label="Device Type"
            help="Fixed to MCU for edge computing"
          >
            <Select placeholder="MCU" disabled>
              <Option value="MCU">MCU</Option>
            </Select>
          </Form.Item>

          <Form.Item
            name="mcuType"
            label="MCU Type"
            help="Select the specific MCU for Renode emulation (only Renode-compatible devices are available)"
            rules={[{ required: true, message: 'Please select an MCU type!' }]}
          >
            <Select placeholder="Select MCU Type">
              <Option value="Stm32F746gDisco">STM32F746G Discovery (Cortex-M7) - 🌐 Ethernet, 340MHz, 1MB Flash</Option>
              <Option value="FrdmK64f">FRDM-K64F (Cortex-M4) - 🌐 Ethernet, 120MHz, 1MB Flash</Option>
              <Option value="Esp32DevkitC">ESP32 DevKit C (Xtensa LX6) - 📡 WiFi, 240MHz, 4MB Flash</Option>
              <Option value="Nrf52840DK">nRF52840 DK (Cortex-M4) - 📡 Bluetooth LE, 64MHz, 1MB Flash</Option>
              <Option value="Stm32F4Disco">STM32F4 Discovery (Cortex-M4) - 168MHz, 1MB Flash, Audio</Option>
              <Option value="Mps2An385">MPS2-AN385 (Cortex-M3) - Legacy</Option>
              <Option value="Mps2An386">MPS2-AN386 (Cortex-M3) - Legacy</Option>
              <Option value="Mps2An500">MPS2-AN500 (Cortex-M33) - Legacy</Option>
              <Option value="Mps2An505">MPS2-AN505 (Cortex-M33) - Legacy</Option>
              <Option value="Stm32Vldiscovery">STM32VL Discovery (Cortex-M3) - Legacy</Option>
              <Option value="OlimexStm32H405">Olimex STM32-H405 (Cortex-M4) - Legacy</Option>
              <Option value="RenodeArduinoNano33Ble">Arduino Nano 33 BLE (Cortex-M4) - Legacy</Option>
              <Option value="RenodeStm32F4Discovery">STM32F4 Discovery (Legacy)</Option>
            </Select>
          </Form.Item>

          <Form.Item
            name="gateway"
            label="Target Gateway"
            help="Select which gateway this device should connect to"
            rules={[{ required: true, message: 'Please select a gateway!' }]}
          >
            <Select placeholder="Select Gateway">
              {gateways.map((gateway) => (
                <Option key={gateway.id} value={gateway.id}>
                  {gateway.name} ({gateway.status})
                </Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item>
            <Space>
              <Button 
                type="primary" 
                htmlType="submit"
                loading={creatingDevice}
                disabled={creatingDevice}
              >
                {creatingDevice ? 'Creating Device...' : 'Create Device'}
              </Button>
              <Button 
                onClick={() => {
                  setModalVisible(false);
                  form.resetFields();
                }}
                disabled={creatingDevice}
              >
                Cancel
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="Device Public Key"
        open={publicKeyModalVisible}
        onCancel={() => setPublicKeyModalVisible(false)}
        footer={[
          <Button key="copy" type="primary" onClick={() => {
            navigator.clipboard.writeText(selectedPublicKey);
          }}>
            Copy to Clipboard
          </Button>,
          <Button key="close" onClick={() => setPublicKeyModalVisible(false)}>
            Close
          </Button>
        ]}
        width={700}
      >
        <Typography.Paragraph>
          <strong>Public Key (Base64):</strong>
        </Typography.Paragraph>
        <Typography.Paragraph
          style={{
            backgroundColor: '#f5f5f5',
            padding: '12px',
            borderRadius: '4px',
            wordBreak: 'break-all',
            fontFamily: 'monospace',
            fontSize: '12px',
            maxHeight: '300px',
            overflow: 'auto'
          }}
        >
          {selectedPublicKey || 'No public key available'}
        </Typography.Paragraph>
        <Typography.Paragraph type="secondary" style={{ fontSize: '12px', marginTop: '8px' }}>
          This is the Ed25519 public key generated for this device. It is used for TLS authentication and device identification.
        </Typography.Paragraph>
      </Modal>
    </div>
  );
};

export default DeviceManagement;
